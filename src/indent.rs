use crate::parenthesis;

pub fn next_indent_level(line: &str, indent_width: usize) -> usize {
    let base = line.chars().take_while(|&c| c == ' ').count() / indent_width;
    if parenthesis::PARENTHESIS_LEFTS
        .iter()
        .any(|&c| line.ends_with(c))
    {
        base + 1
    } else {
        base
    }
}
