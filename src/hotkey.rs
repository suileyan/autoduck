use global_hotkey::hotkey::{Code, Modifiers};

/// 解析快捷键字符串为 (Modifiers, Code)
///
/// 支持格式：`Ctrl+Shift+D`、`Alt+F9`、`Win+M` 等
/// 修饰键不区分大小写，按键不区分大小写
/// 返回 None 表示格式无效
pub fn parse_hotkey(s: &str) -> Option<(Modifiers, Code)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let parts: Vec<&str> = s.split('+').collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;

    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }

        let lower = part.to_lowercase();
        if lower == "ctrl" || lower == "control" {
            modifiers |= Modifiers::CONTROL;
        } else if lower == "alt" {
            modifiers |= Modifiers::ALT;
        } else if lower == "shift" {
            modifiers |= Modifiers::SHIFT;
        } else if lower == "win" || lower == "super" || lower == "meta" {
            modifiers |= Modifiers::SUPER;
        } else if i == parts.len() - 1 {
            // 最后一个部分是按键
            code = parse_code(&lower);
            code?;
        } else {
            return None;
        }
    }

    code.map(|c| (modifiers, c))
}

fn parse_code(s: &str) -> Option<Code> {
    // 字母键 A-Z
    if s.len() == 1 {
        let c = s.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(match c {
                'a' => Code::KeyA,
                'b' => Code::KeyB,
                'c' => Code::KeyC,
                'd' => Code::KeyD,
                'e' => Code::KeyE,
                'f' => Code::KeyF,
                'g' => Code::KeyG,
                'h' => Code::KeyH,
                'i' => Code::KeyI,
                'j' => Code::KeyJ,
                'k' => Code::KeyK,
                'l' => Code::KeyL,
                'm' => Code::KeyM,
                'n' => Code::KeyN,
                'o' => Code::KeyO,
                'p' => Code::KeyP,
                'q' => Code::KeyQ,
                'r' => Code::KeyR,
                's' => Code::KeyS,
                't' => Code::KeyT,
                'u' => Code::KeyU,
                'v' => Code::KeyV,
                'w' => Code::KeyW,
                'x' => Code::KeyX,
                'y' => Code::KeyY,
                'z' => Code::KeyZ,
                _ => return None,
            });
        }
        // 数字键 0-9
        if c.is_ascii_digit() {
            return Some(match c {
                '0' => Code::Digit0,
                '1' => Code::Digit1,
                '2' => Code::Digit2,
                '3' => Code::Digit3,
                '4' => Code::Digit4,
                '5' => Code::Digit5,
                '6' => Code::Digit6,
                '7' => Code::Digit7,
                '8' => Code::Digit8,
                '9' => Code::Digit9,
                _ => return None,
            });
        }
    }

    // 功能键 F1-F24
    if let Some(num) = s.strip_prefix('f') {
        if let Ok(n) = num.parse::<u8>() {
            return match n {
                1 => Some(Code::F1),
                2 => Some(Code::F2),
                3 => Some(Code::F3),
                4 => Some(Code::F4),
                5 => Some(Code::F5),
                6 => Some(Code::F6),
                7 => Some(Code::F7),
                8 => Some(Code::F8),
                9 => Some(Code::F9),
                10 => Some(Code::F10),
                11 => Some(Code::F11),
                12 => Some(Code::F12),
                _ => None,
            };
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ctrl_shift_d() {
        let (mods, code) = parse_hotkey("Ctrl+Shift+D").unwrap();
        assert!(mods.contains(Modifiers::CONTROL));
        assert!(mods.contains(Modifiers::SHIFT));
        assert!(!mods.contains(Modifiers::ALT));
        assert_eq!(code, Code::KeyD);
    }

    #[test]
    fn test_parse_alt_f9() {
        let (mods, code) = parse_hotkey("Alt+F9").unwrap();
        assert!(mods.contains(Modifiers::ALT));
        assert!(!mods.contains(Modifiers::CONTROL));
        assert_eq!(code, Code::F9);
    }

    #[test]
    fn test_parse_case_insensitive() {
        let (mods, code) = parse_hotkey("ctrl+SHIFT+d").unwrap();
        assert!(mods.contains(Modifiers::CONTROL));
        assert!(mods.contains(Modifiers::SHIFT));
        assert_eq!(code, Code::KeyD);
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_hotkey("").is_none());
        assert!(parse_hotkey("  ").is_none());
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_hotkey("Foo+Bar").is_none());
    }

    #[test]
    fn test_parse_single_key() {
        // 单个按键无修饰键也应该可以解析
        let (mods, code) = parse_hotkey("F12").unwrap();
        assert!(mods.is_empty());
        assert_eq!(code, Code::F12);
    }
}
