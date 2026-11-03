pub fn compose_delivery_message(fixed_message: &str, cards: &[String]) -> String {
    let fixed_message = fixed_message.trim();
    let cards = cards
        .iter()
        .map(|card| card.trim())
        .filter(|card| !card.is_empty())
        .collect::<Vec<_>>();

    let card_section = if cards.is_empty() {
        String::new()
    } else {
        let lines = cards
            .iter()
            .enumerate()
            .map(|(index, card)| format!("{}. {card}", index + 1))
            .collect::<Vec<_>>()
            .join("\n");
        format!("兑换码：\n{lines}")
    };

    match (fixed_message.is_empty(), card_section.is_empty()) {
        (false, false) => format!("{fixed_message}\n\n{card_section}"),
        (false, true) => fixed_message.to_string(),
        (true, false) => card_section,
        (true, true) => String::new(),
    }
}
