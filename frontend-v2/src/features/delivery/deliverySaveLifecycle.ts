type CompleteDeliverySaveOptions = {
  releaseSaving: () => void
  close: () => void
  notifySuccess: () => void
  refresh: () => Promise<unknown>
}

export function completeDeliverySave(options: CompleteDeliverySaveOptions) {
  options.releaseSaving()
  options.close()
  options.notifySuccess()
  void options.refresh().catch(() => undefined)
}
