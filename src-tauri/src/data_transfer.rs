use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    ptr, thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose, Engine as _};
use chrono::Local;
use libsqlite3_sys as sqlite;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const PACKAGE_KIND: &str = "yuguanjia-account-data";
const PACKAGE_VERSION: u32 = 1;
const IMPORT_CACHE_DIR: &str = "account-data-import-cache";
const PENDING_IMPORT_DIR: &str = "account-data-pending";
const PENDING_IMPORT_FILE: &str = "pending-account-data-import.json";

#[derive(Debug, Clone, Copy)]
struct SectionDefinition {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    db_file: &'static str,
    required: bool,
}

const SECTION_DEFINITIONS: &[SectionDefinition] = &[
    SectionDefinition {
        key: "accounts",
        label: "账号数据",
        description: "账号、Cookie、账号配置和自动化基础配置",
        db_file: "accounts.db",
        required: true,
    },
    SectionDefinition {
        key: "market",
        label: "商品订单",
        description: "商品缓存、订单缓存、任务和操作记录",
        db_file: "xianyu.db",
        required: false,
    },
    SectionDefinition {
        key: "messages",
        label: "消息记录",
        description: "会话列表、消息缓存和消息连接状态",
        db_file: "live_state.db",
        required: false,
    },
    SectionDefinition {
        key: "notifications",
        label: "通知配置",
        description: "飞书、微信等通知渠道和绑定配置",
        db_file: "notifications.db",
        required: false,
    },
];

fn selected_section_keys(definitions: &[&SectionDefinition]) -> Vec<&'static str> {
    definitions
        .iter()
        .map(|definition| definition.key)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDataSectionPresentation {
    pub key: String,
    pub label: String,
    pub description: String,
    pub required: bool,
    pub present: bool,
    pub size_bytes: u64,
    pub selected_by_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDataTransferPreview {
    pub owner_username: String,
    pub sections: Vec<AccountDataSectionPresentation>,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDataExportResult {
    pub path: String,
    pub file_name: String,
    pub owner_username: String,
    pub sections: Vec<AccountDataSectionPresentation>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDataImportInspection {
    pub import_token: String,
    pub package_owner_username: String,
    pub current_username: String,
    pub can_import: bool,
    pub blocking_reason: Option<String>,
    pub sections: Vec<AccountDataSectionPresentation>,
    pub exported_at: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDataRestoreResult {
    pub restart_required: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct AccountDataPackageBytes {
    pub bytes: Vec<u8>,
    pub owner_username: String,
    pub sections: Vec<AccountDataSectionPresentation>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountDataPackageManifest {
    kind: String,
    version: u32,
    app_version: String,
    owner_username: String,
    exported_at: String,
    sections: Vec<AccountDataPackageSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountDataPackageSection {
    key: String,
    label: String,
    description: String,
    required: bool,
    file_name: String,
    entry_path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingAccountDataImport {
    token: String,
    owner_username: String,
    created_at: String,
    sections: Vec<String>,
}

#[derive(Debug, Clone)]
struct OwnerDataScope {
    account_uids: Vec<String>,
    cookie_scope_keys: Vec<String>,
}

pub fn account_data_transfer_preview(
    data_dir: &Path,
    owner_username: &str,
) -> AccountDataTransferPreview {
    let sections = SECTION_DEFINITIONS
        .iter()
        .map(|definition| section_preview(data_dir, definition, definition.required))
        .collect::<Vec<_>>();
    let total_size_bytes = sections.iter().map(|section| section.size_bytes).sum();

    AccountDataTransferPreview {
        owner_username: owner_username.trim().to_string(),
        sections,
        total_size_bytes,
    }
}

pub fn export_account_data_package(
    data_dir: &Path,
    output_dir: &Path,
    owner_username: &str,
    selected_sections: &[String],
) -> Result<AccountDataExportResult, String> {
    fs::create_dir_all(output_dir).map_err(|error| format!("创建导出目录失败：{error}"))?;
    let file_name = default_export_file_name(owner_username);
    let output_path = output_dir.join(&file_name);
    export_account_data_package_to_path(data_dir, &output_path, owner_username, selected_sections)
}

pub fn export_account_data_package_to_path(
    data_dir: &Path,
    output_path: &Path,
    owner_username: &str,
    selected_sections: &[String],
) -> Result<AccountDataExportResult, String> {
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "account-data-transfer",
        action = "export_to_path",
        outcome = "started",
        username = %owner_username.trim(),
        selected_section_count = selected_sections.len(),
        output_file = %output_path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default(),
        "account data export to path started"
    );
    let package = build_account_data_package_bytes(data_dir, owner_username, selected_sections)?;
    let output_path = ensure_export_extension(output_path);
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    }
    fs::write(&output_path, &package.bytes)
        .map_err(|error| format!("写入导出文件失败：{error}"))?;
    let file_name = output_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| default_export_file_name(&package.owner_username));

    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "account-data-transfer",
        action = "export_to_path",
        outcome = "success",
        username = %package.owner_username,
        selected_section_count = package.sections.len(),
        size_bytes = package.size_bytes,
        output_file = %file_name,
        "account data export to path completed"
    );
    Ok(AccountDataExportResult {
        path: output_path.display().to_string(),
        file_name,
        owner_username: package.owner_username,
        sections: package.sections,
        size_bytes: package.size_bytes,
    })
}

pub fn default_export_file_name(owner_username: &str) -> String {
    format!(
        "鱼管家账号数据-{}-{}.ygjdata",
        sanitize_file_name(owner_username),
        Local::now().format("%Y%m%d-%H%M%S")
    )
}

pub fn build_account_data_package_bytes(
    data_dir: &Path,
    owner_username: &str,
    selected_sections: &[String],
) -> Result<AccountDataPackageBytes, String> {
    let owner_username = required_owner_username(owner_username)?;
    let definitions = selected_definitions(selected_sections)?;
    let log_owner_username = owner_username.clone();
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "account-data-transfer",
        action = "build_export_package",
        outcome = "started",
        username = %owner_username,
        selected_section_count = definitions.len(),
        selected_sections = ?selected_section_keys(&definitions),
        "account data package build started"
    );
    let temp_dir = data_dir
        .join(".account-data-export-tmp")
        .join(Uuid::new_v4().to_string());
    let snapshot_dir = temp_dir.join("snapshots");
    fs::create_dir_all(&snapshot_dir).map_err(|error| format!("创建导出快照目录失败：{error}"))?;

    let result = (|| {
        let mut manifest_sections = Vec::new();
        for definition in &definitions {
            let source = data_dir.join(definition.db_file);
            if !source.is_file() {
                return Err(format!("{}不存在，不能导出", definition.label));
            }

            let snapshot_path = snapshot_dir.join(definition.db_file);
            snapshot_sqlite_database(&source, &snapshot_path)?;
            let size_bytes = file_size(&snapshot_path)?;
            let sha256 = file_sha256(&snapshot_path)?;
            manifest_sections.push(AccountDataPackageSection {
                key: definition.key.to_string(),
                label: definition.label.to_string(),
                description: definition.description.to_string(),
                required: definition.required,
                file_name: definition.db_file.to_string(),
                entry_path: format!("data/{}", definition.db_file),
                size_bytes,
                sha256,
            });
        }

        let manifest = AccountDataPackageManifest {
            kind: PACKAGE_KIND.to_string(),
            version: PACKAGE_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            owner_username: owner_username.clone(),
            exported_at: Local::now().to_rfc3339(),
            sections: manifest_sections,
        };

        let output_path = temp_dir.join("package.ygjdata");
        write_package_zip(&output_path, &manifest, &snapshot_dir)?;
        let bytes = fs::read(&output_path).map_err(|error| format!("读取导出文件失败：{error}"))?;
        let size_bytes = bytes.len() as u64;
        let sections = manifest
            .sections
            .iter()
            .map(package_section_preview)
            .collect::<Vec<_>>();

        Ok(AccountDataPackageBytes {
            bytes,
            owner_username,
            sections,
            size_bytes,
        })
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    match &result {
        Ok(package) => info!(
            target: "client.telemetry.action",
            stream = "usage",
            module = "account-data-transfer",
            action = "build_export_package",
            outcome = "success",
            username = %package.owner_username,
            selected_section_count = package.sections.len(),
            size_bytes = package.size_bytes,
            "account data package build completed"
        ),
        Err(error) => warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "account-data-transfer",
            action = "build_export_package",
            outcome = "failed",
            username = %log_owner_username,
            error_code = "build_export_package_failed",
            error = %error,
            "account data package build failed"
        ),
    }
    result
}

pub fn inspect_account_data_package(
    data_dir: &Path,
    owner_username: &str,
    archive_base64: &str,
) -> Result<AccountDataImportInspection, String> {
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "account-data-transfer",
        action = "inspect_import_package",
        outcome = "started",
        username = %owner_username.trim(),
        archive_base64_len = archive_base64.len(),
        "account data import inspection started"
    );
    let bytes = decode_archive_base64(archive_base64)?;
    match cache_account_data_package_bytes(data_dir, owner_username, &bytes) {
        Ok(inspection) => {
            info!(
                target: "client.telemetry.action",
                stream = "usage",
                module = "account-data-transfer",
                action = "inspect_import_package",
                outcome = "success",
                username = %inspection.current_username,
                package_owner = %inspection.package_owner_username,
                can_import = inspection.can_import,
                section_count = inspection.sections.len(),
                "account data import inspection completed"
            );
            Ok(inspection)
        }
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "account-data-transfer",
                action = "inspect_import_package",
                outcome = "failed",
                username = %owner_username.trim(),
                error_code = "inspect_import_package_failed",
                error = %error,
                "account data import inspection failed"
            );
            Err(error)
        }
    }
}

pub fn cache_account_data_package_bytes(
    data_dir: &Path,
    owner_username: &str,
    bytes: &[u8],
) -> Result<AccountDataImportInspection, String> {
    let current_username = owner_username.trim().to_string();
    let token = Uuid::new_v4().to_string();
    let cache_dir = data_dir.join(IMPORT_CACHE_DIR);
    fs::create_dir_all(&cache_dir).map_err(|error| format!("创建导入缓存失败：{error}"))?;
    let archive_path = cache_dir.join(format!("{token}.ygjdata"));
    fs::write(&archive_path, bytes).map_err(|error| format!("保存导入文件失败：{error}"))?;

    let manifest = read_package_manifest(&archive_path)?;
    let blocking_reason =
        validate_manifest_for_current_user(data_dir, &archive_path, &manifest, &current_username)
            .err();
    let can_import = blocking_reason.is_none();

    Ok(AccountDataImportInspection {
        import_token: token,
        package_owner_username: manifest.owner_username,
        current_username,
        can_import: can_import && blocking_reason.is_none(),
        blocking_reason,
        sections: manifest
            .sections
            .iter()
            .map(package_section_preview)
            .collect(),
        exported_at: manifest.exported_at,
        app_version: manifest.app_version,
    })
}

pub fn restore_account_data_package(
    data_dir: &Path,
    owner_username: &str,
    import_token: &str,
    selected_sections: &[String],
) -> Result<AccountDataRestoreResult, String> {
    let owner_username = required_owner_username(owner_username)?;
    let token = sanitize_token(import_token)?;
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "account-data-transfer",
        action = "prepare_import_restore",
        outcome = "started",
        username = %owner_username,
        import_token = %token,
        selected_section_count = selected_sections.len(),
        "account data restore preparation started"
    );
    let log_owner_username = owner_username.clone();
    let log_token = token.clone();
    let result = (|| -> Result<AccountDataRestoreResult, String> {
        let archive_path = data_dir
            .join(IMPORT_CACHE_DIR)
            .join(format!("{token}.ygjdata"));
        if !archive_path.is_file() {
            return Err("导入文件已失效，请重新选择数据包".to_string());
        }

        let manifest = read_package_manifest(&archive_path)?;
        validate_manifest_for_current_user(data_dir, &archive_path, &manifest, &owner_username)?;
        let definitions = selected_definitions(selected_sections)?;
        ensure_manifest_contains_sections(&manifest, &definitions)?;

        let pending_dir = data_dir.join(PENDING_IMPORT_DIR).join(&token);
        let pending_data_dir = pending_dir.join("data");
        let _ = fs::remove_dir_all(&pending_dir);
        fs::create_dir_all(&pending_data_dir)
            .map_err(|error| format!("创建待导入目录失败：{error}"))?;

        let mut archive = open_zip_archive(&archive_path)?;
        for definition in &definitions {
            let section = manifest_section(&manifest, definition.key)?;
            let target = pending_data_dir.join(definition.db_file);
            extract_manifest_section(&mut archive, section, &target)?;
        }

        let pending = PendingAccountDataImport {
            token,
            owner_username,
            created_at: Local::now().to_rfc3339(),
            sections: definitions
                .iter()
                .map(|definition| definition.key.to_string())
                .collect(),
        };
        let pending_path = data_dir.join(PENDING_IMPORT_FILE);
        write_json_file(&pending_path, &pending)?;

        Ok(AccountDataRestoreResult {
            restart_required: true,
            message: "数据包已校验，客户端将重启并加载导入的数据".to_string(),
        })
    })();
    match &result {
        Ok(_) => info!(
            target: "client.telemetry.action",
            stream = "usage",
            module = "account-data-transfer",
            action = "prepare_import_restore",
            outcome = "success",
            username = %log_owner_username,
            import_token = %log_token,
            selected_section_count = selected_sections.len(),
            "account data restore preparation completed"
        ),
        Err(error) => warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "account-data-transfer",
            action = "prepare_import_restore",
            outcome = "failed",
            username = %log_owner_username,
            import_token = %log_token,
            error_code = "prepare_import_restore_failed",
            error = %error,
            "account data restore preparation failed"
        ),
    }
    result
}

pub fn apply_pending_account_data_import(data_dir: &Path) -> Result<Option<Vec<String>>, String> {
    let pending_path = data_dir.join(PENDING_IMPORT_FILE);
    if !pending_path.is_file() {
        return Ok(None);
    }
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "account-data-transfer",
        action = "apply_pending_import",
        outcome = "started",
        "pending account data import apply started"
    );

    let result = (|| -> Result<Option<(Vec<String>, String)>, String> {
        let text = fs::read_to_string(&pending_path)
            .map_err(|error| format!("读取待导入任务失败：{error}"))?;
        let pending: PendingAccountDataImport =
            serde_json::from_str(&text).map_err(|error| format!("解析待导入任务失败：{error}"))?;
        let token = sanitize_token(&pending.token)?;
        let pending_data_dir = data_dir.join(PENDING_IMPORT_DIR).join(&token).join("data");
        let definitions = selected_definitions(&pending.sections)?;
        let backup_dir = data_dir
            .join("account-data-backups")
            .join(format!("import-{}", timestamp_for_path()));
        fs::create_dir_all(&backup_dir).map_err(|error| format!("创建导入前备份失败：{error}"))?;

        let mut restored = Vec::new();
        for definition in definitions {
            let source = pending_data_dir.join(definition.db_file);
            if !source.is_file() {
                return Err(format!("待导入文件缺失：{}", definition.db_file));
            }
            backup_existing_triplet(data_dir, &backup_dir, definition.db_file)?;
            merge_sqlite_database(&source, &data_dir.join(definition.db_file))
                .map_err(|error| format!("恢复{}失败：{error}", definition.label))?;
            restored.push(definition.label.to_string());
        }

        let _ = fs::remove_file(&pending_path);
        let _ = fs::remove_dir_all(data_dir.join(PENDING_IMPORT_DIR).join(token));
        Ok(Some((restored, pending.owner_username)))
    })();
    match result {
        Ok(Some((restored, owner_username))) => {
            info!(
                target: "client.telemetry.action",
                stream = "usage",
                module = "account-data-transfer",
                action = "apply_pending_import",
                outcome = "success",
                username = %owner_username,
                restored_count = restored.len(),
                restored_sections = ?restored,
                "pending account data import apply completed"
            );
            Ok(Some(restored))
        }
        Ok(None) => Ok(None),
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "account-data-transfer",
                action = "apply_pending_import",
                outcome = "failed",
                error_code = "apply_pending_import_failed",
                error = %error,
                "pending account data import apply failed"
            );
            Err(error)
        }
    }
}

fn section_preview(
    data_dir: &Path,
    definition: &SectionDefinition,
    selected_by_default: bool,
) -> AccountDataSectionPresentation {
    let main = data_dir.join(definition.db_file);
    AccountDataSectionPresentation {
        key: definition.key.to_string(),
        label: definition.label.to_string(),
        description: definition.description.to_string(),
        required: definition.required,
        present: main.is_file(),
        size_bytes: sqlite_triplet_size(data_dir, definition.db_file),
        selected_by_default,
    }
}

fn package_section_preview(section: &AccountDataPackageSection) -> AccountDataSectionPresentation {
    AccountDataSectionPresentation {
        key: section.key.clone(),
        label: section.label.clone(),
        description: section.description.clone(),
        required: section.required,
        present: true,
        size_bytes: section.size_bytes,
        selected_by_default: section.required,
    }
}

fn selected_definitions(
    selected_sections: &[String],
) -> Result<Vec<&'static SectionDefinition>, String> {
    let mut selected = selected_sections
        .iter()
        .map(|key| key.trim())
        .filter(|key| !key.is_empty())
        .collect::<HashSet<_>>();
    selected.insert("accounts");

    let mut definitions = Vec::new();
    for definition in SECTION_DEFINITIONS {
        if selected.contains(definition.key) {
            definitions.push(definition);
        }
    }

    let unknown = selected
        .into_iter()
        .filter(|key| {
            !SECTION_DEFINITIONS
                .iter()
                .any(|definition| definition.key == *key)
        })
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(format!("未知的数据类型：{}", unknown.join("、")));
    }

    Ok(definitions)
}

fn ensure_manifest_contains_sections(
    manifest: &AccountDataPackageManifest,
    definitions: &[&SectionDefinition],
) -> Result<(), String> {
    for definition in definitions {
        if manifest
            .sections
            .iter()
            .all(|section| section.key != definition.key)
        {
            return Err(format!("数据包不包含{}", definition.label));
        }
    }
    Ok(())
}

fn manifest_section<'a>(
    manifest: &'a AccountDataPackageManifest,
    key: &str,
) -> Result<&'a AccountDataPackageSection, String> {
    manifest
        .sections
        .iter()
        .find(|section| section.key == key)
        .ok_or_else(|| format!("数据包不包含 {key}"))
}

fn required_owner_username(owner_username: &str) -> Result<String, String> {
    let owner = owner_username.trim();
    if owner.is_empty() {
        return Err("当前未登录鱼管家账号，不能导入导出账号数据".to_string());
    }
    Ok(owner.to_string())
}

fn validate_manifest_for_current_user(
    _data_dir: &Path,
    archive_path: &Path,
    manifest: &AccountDataPackageManifest,
    _current_username: &str,
) -> Result<(), String> {
    if manifest.kind != PACKAGE_KIND || manifest.version != PACKAGE_VERSION {
        return Err("不是当前版本支持的鱼管家账号数据包".to_string());
    }
    let _ = archive_path;
    Ok(())
}

fn write_package_zip(
    output_path: &Path,
    manifest: &AccountDataPackageManifest,
    snapshot_dir: &Path,
) -> Result<(), String> {
    let file = File::create(output_path).map_err(|error| format!("创建导出文件失败：{error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let manifest_json = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("生成导出清单失败：{error}"))?;

    writer
        .start_file("manifest.json", options)
        .map_err(|error| format!("写入导出清单失败：{error}"))?;
    writer
        .write_all(&manifest_json)
        .map_err(|error| format!("写入导出清单失败：{error}"))?;

    for section in &manifest.sections {
        writer
            .start_file(&section.entry_path, options)
            .map_err(|error| format!("写入{}失败：{error}", section.label))?;
        let mut source = File::open(snapshot_dir.join(&section.file_name))
            .map_err(|error| format!("读取{}快照失败：{error}", section.label))?;
        io::copy(&mut source, &mut writer)
            .map_err(|error| format!("写入{}失败：{error}", section.label))?;
    }

    writer
        .finish()
        .map_err(|error| format!("完成导出文件失败：{error}"))?;
    Ok(())
}

fn open_zip_archive(path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(path).map_err(|error| format!("打开数据包失败：{error}"))?;
    ZipArchive::new(file).map_err(|error| format!("读取数据包失败：{error}"))
}

fn read_package_manifest(path: &Path) -> Result<AccountDataPackageManifest, String> {
    let mut archive = open_zip_archive(path)?;
    let mut manifest_file = archive
        .by_name("manifest.json")
        .map_err(|_| "数据包缺少 manifest.json".to_string())?;
    let mut text = String::new();
    manifest_file
        .read_to_string(&mut text)
        .map_err(|error| format!("读取数据包清单失败：{error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("解析数据包清单失败：{error}"))
}

fn extract_manifest_section(
    archive: &mut ZipArchive<File>,
    section: &AccountDataPackageSection,
    target: &Path,
) -> Result<(), String> {
    let mut entry = archive
        .by_name(&section.entry_path)
        .map_err(|_| format!("数据包缺少{}", section.label))?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建导入目录失败：{error}"))?;
    }
    let mut output =
        File::create(target).map_err(|error| format!("写入{}失败：{error}", section.label))?;
    io::copy(&mut entry, &mut output)
        .map_err(|error| format!("解压{}失败：{error}", section.label))?;
    let sha256 = file_sha256(target)?;
    if sha256 != section.sha256 {
        return Err(format!("{}校验失败，数据包可能已损坏", section.label));
    }
    Ok(())
}

fn snapshot_sqlite_database(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建快照目录失败：{error}"))?;
    }
    let _ = fs::remove_file(destination);
    unsafe {
        let mut source_db: *mut sqlite::sqlite3 = ptr::null_mut();
        let mut destination_db: *mut sqlite::sqlite3 = ptr::null_mut();
        let source_c = path_to_cstring(source)?;
        let destination_c = path_to_cstring(destination)?;

        let source_rc = sqlite::sqlite3_open_v2(
            source_c.as_ptr(),
            &mut source_db,
            sqlite::SQLITE_OPEN_READONLY,
            ptr::null(),
        );
        if source_rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(source_db);
            close_sqlite(source_db);
            return Err(format!("打开数据库快照源失败：{message}"));
        }

        let destination_rc = sqlite::sqlite3_open_v2(
            destination_c.as_ptr(),
            &mut destination_db,
            sqlite::SQLITE_OPEN_READWRITE | sqlite::SQLITE_OPEN_CREATE,
            ptr::null(),
        );
        if destination_rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(destination_db);
            close_sqlite(source_db);
            close_sqlite(destination_db);
            return Err(format!("创建数据库快照失败：{message}"));
        }

        let main = CString::new("main").unwrap();
        let backup =
            sqlite::sqlite3_backup_init(destination_db, main.as_ptr(), source_db, main.as_ptr());
        if backup.is_null() {
            let message = sqlite_error_message(destination_db);
            close_sqlite(source_db);
            close_sqlite(destination_db);
            return Err(format!("初始化数据库快照失败：{message}"));
        }

        let mut step_rc;
        let mut busy_retries = 0;
        loop {
            step_rc = sqlite::sqlite3_backup_step(backup, 256);
            if step_rc == sqlite::SQLITE_DONE {
                break;
            }
            if step_rc == sqlite::SQLITE_OK {
                continue;
            }
            if matches!(step_rc, sqlite::SQLITE_BUSY | sqlite::SQLITE_LOCKED) && busy_retries < 80 {
                busy_retries += 1;
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            break;
        }
        let finish_rc = sqlite::sqlite3_backup_finish(backup);
        let close_source_rc = sqlite::sqlite3_close(source_db);
        let close_destination_rc = sqlite::sqlite3_close(destination_db);

        if step_rc != sqlite::SQLITE_DONE {
            return Err(format!("生成数据库快照失败：SQLite code {step_rc}"));
        }
        if finish_rc != sqlite::SQLITE_OK {
            return Err(format!("完成数据库快照失败：SQLite code {finish_rc}"));
        }
        if close_source_rc != sqlite::SQLITE_OK || close_destination_rc != sqlite::SQLITE_OK {
            return Err("关闭数据库快照连接失败".to_string());
        }
    }
    Ok(())
}

fn owner_scope_from_accounts_snapshot(
    path: &Path,
    owner_username: &str,
) -> Result<Option<OwnerDataScope>, String> {
    unsafe {
        let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
        let path_c = path_to_cstring(path)?;
        let rc = sqlite::sqlite3_open_v2(
            path_c.as_ptr(),
            &mut db,
            sqlite::SQLITE_OPEN_READONLY,
            ptr::null(),
        );
        if rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(db);
            close_sqlite(db);
            return Err(format!("读取账号导出范围失败：{message}"));
        }

        let result = (|| {
            let tables = sqlite_user_tables(db)?;
            if !tables.iter().any(|table| table == "accounts") {
                return Ok(None);
            }
            let columns = sqlite_table_columns(db, "accounts")?;
            if !columns.contains("owner_username") || !columns.contains("uid") {
                return Err("账号数据缺少归属字段，不能导出".to_string());
            }

            let owner = sqlite_quote(owner_username.trim());
            let account_uids = sqlite_string_column_values(
                db,
                &format!(
                    "SELECT DISTINCT uid FROM {} \
                     WHERE TRIM(COALESCE(owner_username, '')) = {owner} \
                       AND TRIM(COALESCE(uid, '')) <> '' \
                     ORDER BY uid",
                    sqlite_ident("accounts"),
                ),
            )?;
            if account_uids.is_empty() {
                return Err("当前鱼管家账号下没有可导出的闲鱼账号数据".to_string());
            }

            let mut cookie_scope_keys = account_uids.clone();
            cookie_scope_keys.push(user_global_scope_key(owner_username));
            cookie_scope_keys.sort();
            cookie_scope_keys.dedup();

            Ok(Some(OwnerDataScope {
                account_uids,
                cookie_scope_keys,
            }))
        })();

        close_sqlite(db);
        result
    }
}

fn prune_sqlite_snapshot_for_owner(
    path: &Path,
    owner_username: &str,
    scope: &OwnerDataScope,
) -> Result<(), String> {
    unsafe {
        let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
        let path_c = path_to_cstring(path)?;
        let rc = sqlite::sqlite3_open_v2(
            path_c.as_ptr(),
            &mut db,
            sqlite::SQLITE_OPEN_READWRITE,
            ptr::null(),
        );
        if rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(db);
            close_sqlite(db);
            return Err(format!("裁剪导出数据失败：{message}"));
        }

        let result = (|| {
            sqlite_exec(db, "PRAGMA foreign_keys = OFF")?;
            let tables = sqlite_user_tables(db)?;
            let owner = sqlite_quote(owner_username.trim());
            let uid_list = sqlite_string_list(&scope.account_uids);
            let cookie_list = sqlite_string_list(&scope.cookie_scope_keys);

            for table in &tables {
                if table == "_sqlx_migrations" {
                    continue;
                }
                let columns = sqlite_table_columns(db, table)?;
                let table_name = sqlite_ident(table);

                if columns.contains("owner_username") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(owner_username, '')) <> {owner}"
                        ),
                    )?;
                }
                if columns.contains("requester_username") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(requester_username, '')) <> {owner}"
                        ),
                    )?;
                }
                if columns.contains("username") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(username, '')) <> '' \
                               AND TRIM(COALESCE(username, '')) <> {owner}"
                        ),
                    )?;
                }
                if columns.contains("operator") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(operator, '')) <> '' \
                               AND TRIM(COALESCE(operator, '')) <> {owner}"
                        ),
                    )?;
                }
                if columns.contains("uid") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(uid, '')) <> '' \
                               AND uid NOT IN {uid_list}"
                        ),
                    )?;
                }
                if columns.contains("account_uid") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(account_uid, '')) <> '' \
                               AND account_uid NOT IN {uid_list}"
                        ),
                    )?;
                }
                if columns.contains("cookie_id") {
                    sqlite_exec(
                        db,
                        &format!(
                            "DELETE FROM {table_name} \
                             WHERE TRIM(COALESCE(cookie_id, '')) = '' \
                                OR cookie_id NOT IN {cookie_list}"
                        ),
                    )?;
                }
            }

            prune_related_tables(db, &tables)?;
            sqlite_exec(db, "PRAGMA optimize")?;
            Ok(())
        })();

        close_sqlite(db);
        result
    }
}

unsafe fn prune_related_tables(db: *mut sqlite::sqlite3, tables: &[String]) -> Result<(), String> {
    let has_table = |name: &str| tables.iter().any(|table| table == name);

    if has_table("notification_channels") {
        for table in tables {
            if table == "notification_channels" || table == "_sqlx_migrations" {
                continue;
            }
            let columns = sqlite_table_columns(db, table)?;
            if columns.contains("channel_id") {
                sqlite_exec(
                    db,
                    &format!(
                        "DELETE FROM {} WHERE channel_id NOT IN (SELECT id FROM {})",
                        sqlite_ident(table),
                        sqlite_ident("notification_channels"),
                    ),
                )?;
            }
        }
    }

    if has_table("comment_tasks") && has_table("comment_task_accounts") {
        sqlite_exec(
            db,
            "DELETE FROM comment_tasks \
             WHERE task_id NOT IN (SELECT DISTINCT task_id FROM comment_task_accounts)",
        )?;
        for table in tables {
            if matches!(table.as_str(), "comment_tasks" | "_sqlx_migrations") {
                continue;
            }
            let columns = sqlite_table_columns(db, table)?;
            if columns.contains("task_id") {
                sqlite_exec(
                    db,
                    &format!(
                        "DELETE FROM {} WHERE task_id NOT IN (SELECT task_id FROM comment_tasks)",
                        sqlite_ident(table),
                    ),
                )?;
            }
        }
    }

    if has_table("operation_jobs") {
        for table in tables {
            if matches!(table.as_str(), "operation_jobs" | "_sqlx_migrations") {
                continue;
            }
            let columns = sqlite_table_columns(db, table)?;
            if columns.contains("job_id") {
                sqlite_exec(
                    db,
                    &format!(
                        "DELETE FROM {} \
                         WHERE TRIM(COALESCE(job_id, '')) <> '' \
                           AND job_id NOT IN (SELECT id FROM operation_jobs)",
                        sqlite_ident(table),
                    ),
                )?;
            }
        }
    }

    if has_table("cache_sync_runtime_state") {
        sqlite_exec(
            db,
            "UPDATE cache_sync_runtime_state \
             SET status = 'idle', cached_uids_json = '[]', total_entries = 0, last_sync_at = 0, updated_at = 0",
        )?;
    }

    Ok(())
}

fn read_account_owner_usernames(path: &Path) -> Result<Vec<String>, String> {
    unsafe {
        let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
        let path_c = path_to_cstring(path)?;
        let rc = sqlite::sqlite3_open_v2(
            path_c.as_ptr(),
            &mut db,
            sqlite::SQLITE_OPEN_READONLY,
            ptr::null(),
        );
        if rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(db);
            close_sqlite(db);
            return Err(format!("打开账号数据失败：{message}"));
        }

        let sql = CString::new(
            "SELECT DISTINCT COALESCE(owner_username, '') \
             FROM accounts \
             WHERE COALESCE(owner_username, '') <> '' \
             LIMIT 20",
        )
        .unwrap();
        let mut stmt: *mut sqlite::sqlite3_stmt = ptr::null_mut();
        let prepare_rc =
            sqlite::sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, ptr::null_mut());
        if prepare_rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(db);
            close_sqlite(db);
            return Err(format!("读取账号归属失败：{message}"));
        }

        let mut owners = Vec::new();
        loop {
            let step_rc = sqlite::sqlite3_step(stmt);
            if step_rc == sqlite::SQLITE_ROW {
                let text = sqlite::sqlite3_column_text(stmt, 0);
                if !text.is_null() {
                    owners.push(
                        CStr::from_ptr(text.cast())
                            .to_string_lossy()
                            .trim()
                            .to_string(),
                    );
                }
                continue;
            }
            if step_rc == sqlite::SQLITE_DONE {
                break;
            }
            let message = sqlite_error_message(db);
            sqlite::sqlite3_finalize(stmt);
            close_sqlite(db);
            return Err(format!("读取账号归属失败：{message}"));
        }
        sqlite::sqlite3_finalize(stmt);
        close_sqlite(db);
        owners.sort();
        owners.dedup();
        Ok(owners
            .into_iter()
            .filter(|owner| !owner.is_empty())
            .collect())
    }
}

fn merge_sqlite_database(source: &Path, destination: &Path) -> Result<(), String> {
    if !destination.is_file() {
        fs::copy(source, destination).map_err(|error| format!("复制数据库失败：{error}"))?;
        return Ok(());
    }

    unsafe {
        let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
        let destination_c = path_to_cstring(destination)?;
        let rc = sqlite::sqlite3_open_v2(
            destination_c.as_ptr(),
            &mut db,
            sqlite::SQLITE_OPEN_READWRITE,
            ptr::null(),
        );
        if rc != sqlite::SQLITE_OK {
            let message = sqlite_error_message(db);
            close_sqlite(db);
            return Err(format!("打开目标数据库失败：{message}"));
        }

        let source_path = sqlite_quote(&source.to_string_lossy());
        let result = (|| {
            sqlite_exec(db, "PRAGMA foreign_keys = OFF")?;
            sqlite_exec(db, &format!("ATTACH DATABASE {source_path} AS incoming"))?;
            sqlite_exec(db, "BEGIN IMMEDIATE")?;
            let merge_result = (|| {
                let tables = sqlite_user_tables_in_schema(db, "incoming")?;
                for table in tables {
                    ensure_destination_table_exists(db, table.as_str())?;
                    merge_sqlite_table(db, table.as_str())?;
                }
                Ok(())
            })();

            match merge_result {
                Ok(()) => {
                    sqlite_exec(db, "COMMIT")?;
                    sqlite_exec(db, "DETACH DATABASE incoming")
                }
                Err(error) => {
                    let _ = sqlite_exec(db, "ROLLBACK");
                    let _ = sqlite_exec(db, "DETACH DATABASE incoming");
                    Err(error)
                }
            }
        })();

        close_sqlite(db);
        result
    }
}

unsafe fn ensure_destination_table_exists(
    db: *mut sqlite::sqlite3,
    table: &str,
) -> Result<(), String> {
    let exists = sqlite_string_column_values(
        db,
        &format!(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = {} LIMIT 1",
            sqlite_quote(table),
        ),
    )?;
    if !exists.is_empty() {
        return Ok(());
    }

    let create_sql = sqlite_string_column_values(
        db,
        &format!(
            "SELECT sql FROM incoming.sqlite_master \
             WHERE type = 'table' AND name = {} AND sql IS NOT NULL LIMIT 1",
            sqlite_quote(table),
        ),
    )?
    .into_iter()
    .next()
    .ok_or_else(|| format!("导入数据缺少表结构：{table}"))?;
    sqlite_exec(db, &create_sql)
}

unsafe fn merge_sqlite_table(db: *mut sqlite::sqlite3, table: &str) -> Result<(), String> {
    let incoming_columns = sqlite_table_column_names_in_schema(db, "incoming", table)?;
    let destination_columns = sqlite_table_columns(db, table)?;
    let columns = incoming_columns
        .into_iter()
        .filter(|column| destination_columns.contains(column))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return Ok(());
    }

    let column_list = columns
        .iter()
        .map(|column| sqlite_ident(column))
        .collect::<Vec<_>>()
        .join(", ");
    sqlite_exec(
        db,
        &format!(
            "INSERT OR IGNORE INTO {} ({column_list}) \
             SELECT {column_list} FROM {}.{}",
            sqlite_ident(table),
            sqlite_ident("incoming"),
            sqlite_ident(table),
        ),
    )
}

unsafe fn sqlite_user_tables(db: *mut sqlite::sqlite3) -> Result<Vec<String>, String> {
    sqlite_user_tables_in_schema(db, "main")
}

unsafe fn sqlite_user_tables_in_schema(
    db: *mut sqlite::sqlite3,
    schema: &str,
) -> Result<Vec<String>, String> {
    sqlite_string_column_values(
        db,
        &format!(
            "SELECT name FROM {}.sqlite_master \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         ORDER BY name",
            sqlite_ident(schema),
        ),
    )
}

unsafe fn sqlite_table_columns(
    db: *mut sqlite::sqlite3,
    table: &str,
) -> Result<HashSet<String>, String> {
    let rows = sqlite_table_column_names_in_schema(db, "main", table)?;
    Ok(rows.into_iter().collect())
}

unsafe fn sqlite_table_column_names_in_schema(
    db: *mut sqlite::sqlite3,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, String> {
    sqlite_string_column_values_at(
        db,
        &format!(
            "PRAGMA {}.table_info({})",
            sqlite_ident(schema),
            sqlite_ident(table),
        ),
        1,
    )
}

unsafe fn sqlite_string_column_values(
    db: *mut sqlite::sqlite3,
    sql: &str,
) -> Result<Vec<String>, String> {
    sqlite_string_column_values_at(db, sql, 0)
}

unsafe fn sqlite_string_column_values_at(
    db: *mut sqlite::sqlite3,
    sql: &str,
    column_index: i32,
) -> Result<Vec<String>, String> {
    let sql_c = CString::new(sql).map_err(|_| "SQL 包含非法字符".to_string())?;
    let mut stmt: *mut sqlite::sqlite3_stmt = ptr::null_mut();
    let prepare_rc = sqlite::sqlite3_prepare_v2(db, sql_c.as_ptr(), -1, &mut stmt, ptr::null_mut());
    if prepare_rc != sqlite::SQLITE_OK {
        let message = sqlite_error_message(db);
        return Err(format!("读取数据库元数据失败：{message}"));
    }

    let mut values = Vec::new();
    loop {
        let step_rc = sqlite::sqlite3_step(stmt);
        if step_rc == sqlite::SQLITE_ROW {
            let text = sqlite::sqlite3_column_text(stmt, column_index);
            if !text.is_null() {
                values.push(
                    CStr::from_ptr(text.cast())
                        .to_string_lossy()
                        .trim()
                        .to_string(),
                );
            }
            continue;
        }
        if step_rc == sqlite::SQLITE_DONE {
            break;
        }
        let message = sqlite_error_message(db);
        sqlite::sqlite3_finalize(stmt);
        return Err(format!("读取数据库元数据失败：{message}"));
    }
    sqlite::sqlite3_finalize(stmt);
    Ok(values)
}

unsafe fn sqlite_exec(db: *mut sqlite::sqlite3, sql: &str) -> Result<(), String> {
    let sql_c = CString::new(sql).map_err(|_| "SQL 包含非法字符".to_string())?;
    let mut error_message: *mut std::os::raw::c_char = ptr::null_mut();
    let rc = sqlite::sqlite3_exec(
        db,
        sql_c.as_ptr(),
        None,
        ptr::null_mut(),
        &mut error_message,
    );
    if rc == sqlite::SQLITE_OK {
        return Ok(());
    }

    let message = if error_message.is_null() {
        sqlite_error_message(db)
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        sqlite::sqlite3_free(error_message.cast());
        message
    };
    Err(format!("裁剪导出数据失败：{message}"))
}

fn sqlite_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sqlite_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn sqlite_string_list(values: &[String]) -> String {
    if values.is_empty() {
        "(NULL)".to_string()
    } else {
        format!(
            "({})",
            values
                .iter()
                .map(|value| sqlite_quote(value))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn user_global_scope_key(owner_username: &str) -> String {
    let owner = owner_username.trim();
    if owner.is_empty() {
        "__global__".to_string()
    } else {
        format!("__global__::user::{owner}")
    }
}

fn path_to_cstring(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| format!("路径包含非法字符：{}", path.display()))
}

unsafe fn sqlite_error_message(db: *mut sqlite::sqlite3) -> String {
    if db.is_null() {
        return "unknown sqlite error".to_string();
    }
    CStr::from_ptr(sqlite::sqlite3_errmsg(db))
        .to_string_lossy()
        .into_owned()
}

unsafe fn close_sqlite(db: *mut sqlite::sqlite3) {
    if !db.is_null() {
        let _ = sqlite::sqlite3_close(db);
    }
}

fn file_size(path: &Path) -> Result<u64, String> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("读取文件大小失败：{error}"))
}

fn sqlite_triplet_size(data_dir: &Path, db_file: &str) -> u64 {
    sqlite_triplet_paths(data_dir, db_file)
        .iter()
        .filter_map(|path| fs::metadata(path).ok().map(|metadata| metadata.len()))
        .sum()
}

fn sqlite_triplet_paths(data_dir: &Path, db_file: &str) -> [PathBuf; 3] {
    [
        data_dir.join(db_file),
        data_dir.join(format!("{db_file}-wal")),
        data_dir.join(format!("{db_file}-shm")),
    ]
}

fn backup_existing_triplet(
    data_dir: &Path,
    backup_dir: &Path,
    db_file: &str,
) -> Result<(), String> {
    fs::create_dir_all(backup_dir).map_err(|error| format!("创建导入前备份失败：{error}"))?;
    for path in sqlite_triplet_paths(data_dir, db_file) {
        if path.is_file() {
            let Some(file_name) = path.file_name() else {
                continue;
            };
            fs::copy(&path, backup_dir.join(file_name))
                .map_err(|error| format!("备份{}失败：{error}", path.display()))?;
        }
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("读取文件失败：{error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|error| format!("读取文件失败：{error}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| format!("生成 JSON 失败：{error}"))?;
    fs::write(&tmp, bytes).map_err(|error| format!("写入 JSON 失败：{error}"))?;
    fs::rename(&tmp, path).map_err(|error| format!("保存 JSON 失败：{error}"))
}

fn decode_archive_base64(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    let payload = trimmed
        .split_once(',')
        .map(|(_, value)| value)
        .unwrap_or(trimmed)
        .trim();
    general_purpose::STANDARD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(payload))
        .map_err(|error| format!("解析数据包失败：{error}"))
}

fn sanitize_file_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "user".to_string()
    } else {
        trimmed.to_string()
    }
}

fn ensure_export_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ygjdata"))
    {
        path.to_path_buf()
    } else {
        path.with_extension("ygjdata")
    }
}

fn sanitize_token(value: &str) -> Result<String, String> {
    let token = value.trim();
    if token.len() > 80
        || token.is_empty()
        || !token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err("导入任务标识无效，请重新选择数据包".to_string());
    }
    Ok(token.to_string())
}

fn timestamp_for_path() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn selected_sections_always_include_accounts() {
        let definitions = selected_definitions(&["market".to_string()]).unwrap();
        let keys = definitions
            .iter()
            .map(|definition| definition.key)
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["accounts", "market"]);
    }

    #[test]
    fn selected_sections_reject_unknown_keys() {
        let error =
            selected_definitions(&["accounts".to_string(), "license".to_string()]).unwrap_err();
        assert!(error.contains("未知的数据类型"));
    }

    #[test]
    fn preview_selects_only_required_accounts_by_default() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("accounts.db"), b"accounts").unwrap();
        fs::write(temp.path().join("xianyu.db"), b"market").unwrap();

        let preview = account_data_transfer_preview(temp.path(), "alice");
        let accounts = preview
            .sections
            .iter()
            .find(|section| section.key == "accounts")
            .unwrap();
        let market = preview
            .sections
            .iter()
            .find(|section| section.key == "market")
            .unwrap();

        assert!(accounts.required);
        assert!(accounts.present);
        assert!(accounts.selected_by_default);
        assert!(!market.required);
        assert!(market.present);
        assert!(!market.selected_by_default);
    }

    #[test]
    fn build_package_bytes_always_include_accounts() {
        let temp = tempdir().unwrap();
        create_empty_sqlite(&temp.path().join("accounts.db"));
        create_empty_sqlite(&temp.path().join("xianyu.db"));

        let package =
            build_account_data_package_bytes(temp.path(), "alice", &["market".to_string()])
                .unwrap();
        let keys = package
            .sections
            .iter()
            .map(|section| section.key.as_str())
            .collect::<Vec<_>>();

        assert_eq!(keys, vec!["accounts", "market"]);
        assert_eq!(package.owner_username, "alice");
        assert_eq!(package.size_bytes, package.bytes.len() as u64);
        assert!(!package.bytes.is_empty());
    }

    #[test]
    fn build_package_bytes_includes_all_local_data_regardless_of_owner_label() {
        let temp = tempdir().unwrap();
        create_sqlite_with_statements(
            &temp.path().join("accounts.db"),
            &[
                "CREATE TABLE accounts (uid TEXT PRIMARY KEY, owner_username TEXT NOT NULL DEFAULT '')",
                "CREATE TABLE items (item_id TEXT PRIMARY KEY, uid TEXT NOT NULL)",
                "CREATE TABLE ai_reply_settings (cookie_id TEXT PRIMARY KEY, model_name TEXT DEFAULT '')",
                "INSERT INTO accounts (uid, owner_username) VALUES ('alice-uid', 'alice'), ('bob-uid', 'bob')",
                "INSERT INTO items (item_id, uid) VALUES ('alice-item', 'alice-uid'), ('bob-item', 'bob-uid')",
                "INSERT INTO ai_reply_settings (cookie_id, model_name) VALUES ('__global__::user::alice', 'alice-model'), ('__global__::user::bob', 'bob-model')",
            ],
        );
        create_sqlite_with_statements(
            &temp.path().join("xianyu.db"),
            &[
                "CREATE TABLE account_items (uid TEXT NOT NULL, item_id TEXT NOT NULL, PRIMARY KEY (uid, item_id))",
                "CREATE TABLE automation_schedules (id TEXT PRIMARY KEY, owner_username TEXT NOT NULL DEFAULT '')",
                "INSERT INTO account_items (uid, item_id) VALUES ('alice-uid', 'alice-market-item'), ('bob-uid', 'bob-market-item')",
                "INSERT INTO automation_schedules (id, owner_username) VALUES ('alice-task', 'alice'), ('bob-task', 'bob')",
            ],
        );

        let package =
            build_account_data_package_bytes(temp.path(), "alice", &["market".to_string()])
                .unwrap();
        let archive_path = temp.path().join("package.ygjdata");
        fs::write(&archive_path, package.bytes).unwrap();
        let manifest = read_package_manifest(&archive_path).unwrap();
        let accounts = manifest_section(&manifest, "accounts").unwrap();
        let inspect_db = temp.path().join("inspect-accounts.db");
        let mut archive = open_zip_archive(&archive_path).unwrap();
        extract_manifest_section(&mut archive, accounts, &inspect_db).unwrap();
        let market = manifest_section(&manifest, "market").unwrap();
        let inspect_market_db = temp.path().join("inspect-market.db");
        extract_manifest_section(&mut archive, market, &inspect_market_db).unwrap();

        assert_eq!(read_account_owner_usernames(&inspect_db).unwrap(), vec!["alice", "bob"]);
        assert_eq!(
            sqlite_scalar_i64(
                &inspect_db,
                "SELECT COUNT(*) FROM accounts WHERE owner_username = 'bob'"
            ),
            1
        );
        assert_eq!(
            sqlite_scalar_i64(
                &inspect_db,
                "SELECT COUNT(*) FROM items WHERE uid = 'bob-uid'"
            ),
            1
        );
        assert_eq!(
            sqlite_scalar_i64(
                &inspect_db,
                "SELECT COUNT(*) FROM ai_reply_settings WHERE cookie_id = '__global__::user::bob'",
            ),
            1,
        );
        assert_eq!(
            sqlite_scalar_i64(
                &inspect_market_db,
                "SELECT COUNT(*) FROM account_items WHERE uid = 'bob-uid'"
            ),
            1
        );
        assert_eq!(
            sqlite_scalar_i64(
                &inspect_market_db,
                "SELECT COUNT(*) FROM automation_schedules WHERE owner_username = 'bob'",
            ),
            1,
        );
    }

    #[test]
    fn pending_import_merges_accounts_without_removing_existing_accounts() {
        let current = tempdir().unwrap();
        create_sqlite_with_statements(
            &current.path().join("accounts.db"),
            &[
                "CREATE TABLE accounts (uid TEXT PRIMARY KEY, owner_username TEXT NOT NULL DEFAULT '', nick TEXT DEFAULT '')",
                "INSERT INTO accounts (uid, owner_username, nick) VALUES ('new-local-uid', 'alice', '本机新加账号')",
            ],
        );

        let exported = tempdir().unwrap();
        create_sqlite_with_statements(
            &exported.path().join("accounts.db"),
            &[
                "CREATE TABLE accounts (uid TEXT PRIMARY KEY, owner_username TEXT NOT NULL DEFAULT '', nick TEXT DEFAULT '')",
                "INSERT INTO accounts (uid, owner_username, nick) VALUES ('new-local-uid', 'alice', '导入包旧账号名')",
                "INSERT INTO accounts (uid, owner_username, nick) VALUES ('restored-uid', 'alice', '导入恢复账号')",
            ],
        );

        let package = build_account_data_package_bytes(exported.path(), "bob", &[]).unwrap();
        let token = "merge-accounts";
        let cache_dir = current.path().join(IMPORT_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(format!("{token}.ygjdata")), package.bytes).unwrap();

        restore_account_data_package(current.path(), "alice", token, &[]).unwrap();
        apply_pending_account_data_import(current.path()).unwrap();

        assert_eq!(
            sqlite_scalar_i64(
                &current.path().join("accounts.db"),
                "SELECT COUNT(*) FROM accounts WHERE uid = 'new-local-uid'",
            ),
            1,
        );
        assert_eq!(
            sqlite_scalar_i64(
                &current.path().join("accounts.db"),
                "SELECT COUNT(*) FROM accounts WHERE uid = 'new-local-uid' AND nick = '本机新加账号'",
            ),
            1,
        );
        assert_eq!(
            sqlite_scalar_i64(
                &current.path().join("accounts.db"),
                "SELECT COUNT(*) FROM accounts WHERE uid = 'restored-uid'",
            ),
            1,
        );
    }

    #[test]
    fn sanitize_token_rejects_paths() {
        assert!(sanitize_token("../abc").is_err());
        assert!(sanitize_token("abc/def").is_err());
        assert!(sanitize_token("8f6d-abc").is_ok());
    }

    fn create_empty_sqlite(path: &Path) {
        unsafe {
            let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
            let path_c = path_to_cstring(path).unwrap();
            let rc = sqlite::sqlite3_open_v2(
                path_c.as_ptr(),
                &mut db,
                sqlite::SQLITE_OPEN_READWRITE | sqlite::SQLITE_OPEN_CREATE,
                ptr::null(),
            );
            assert_eq!(rc, sqlite::SQLITE_OK);
            close_sqlite(db);
        }
    }

    fn create_sqlite_with_statements(path: &Path, statements: &[&str]) {
        create_empty_sqlite(path);
        unsafe {
            let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
            let path_c = path_to_cstring(path).unwrap();
            let rc = sqlite::sqlite3_open_v2(
                path_c.as_ptr(),
                &mut db,
                sqlite::SQLITE_OPEN_READWRITE,
                ptr::null(),
            );
            assert_eq!(rc, sqlite::SQLITE_OK);
            for statement in statements {
                exec_sql_for_test(db, statement);
            }
            close_sqlite(db);
        }
    }

    unsafe fn exec_sql_for_test(db: *mut sqlite::sqlite3, statement: &str) {
        let sql = CString::new(statement).unwrap();
        let rc = sqlite::sqlite3_exec(db, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut());
        assert_eq!(rc, sqlite::SQLITE_OK, "SQL failed: {statement}");
    }

    fn sqlite_scalar_i64(path: &Path, sql: &str) -> i64 {
        unsafe {
            let mut db: *mut sqlite::sqlite3 = ptr::null_mut();
            let path_c = path_to_cstring(path).unwrap();
            let rc = sqlite::sqlite3_open_v2(
                path_c.as_ptr(),
                &mut db,
                sqlite::SQLITE_OPEN_READONLY,
                ptr::null(),
            );
            assert_eq!(rc, sqlite::SQLITE_OK);
            let sql_c = CString::new(sql).unwrap();
            let mut stmt: *mut sqlite::sqlite3_stmt = ptr::null_mut();
            let prepare_rc =
                sqlite::sqlite3_prepare_v2(db, sql_c.as_ptr(), -1, &mut stmt, ptr::null_mut());
            assert_eq!(prepare_rc, sqlite::SQLITE_OK);
            let value = if sqlite::sqlite3_step(stmt) == sqlite::SQLITE_ROW {
                sqlite::sqlite3_column_int64(stmt, 0)
            } else {
                0
            };
            sqlite::sqlite3_finalize(stmt);
            close_sqlite(db);
            value
        }
    }
}
