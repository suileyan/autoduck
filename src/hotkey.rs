use global_hotkey::hotkey::{Code, Modifiers};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

/// Unified key mapping entry: (Code, display_name, vk_code)
struct KeyMapping {
    code: Code,
    name: &'static str,
    vk: u16,
}

/// Single source of truth for key ↔ Code ↔ VK mappings.
/// Used by text_to_code, vk_to_code, and format_code.
const KEY_MAP: &[KeyMapping] = &[
    // Letters A-Z (VK_A=0x41 .. VK_Z=0x5A)
    KeyMapping { code: Code::KeyA, name: "A", vk: 0x41 },
    KeyMapping { code: Code::KeyB, name: "B", vk: 0x42 },
    KeyMapping { code: Code::KeyC, name: "C", vk: 0x43 },
    KeyMapping { code: Code::KeyD, name: "D", vk: 0x44 },
    KeyMapping { code: Code::KeyE, name: "E", vk: 0x45 },
    KeyMapping { code: Code::KeyF, name: "F", vk: 0x46 },
    KeyMapping { code: Code::KeyG, name: "G", vk: 0x47 },
    KeyMapping { code: Code::KeyH, name: "H", vk: 0x48 },
    KeyMapping { code: Code::KeyI, name: "I", vk: 0x49 },
    KeyMapping { code: Code::KeyJ, name: "J", vk: 0x4A },
    KeyMapping { code: Code::KeyK, name: "K", vk: 0x4B },
    KeyMapping { code: Code::KeyL, name: "L", vk: 0x4C },
    KeyMapping { code: Code::KeyM, name: "M", vk: 0x4D },
    KeyMapping { code: Code::KeyN, name: "N", vk: 0x4E },
    KeyMapping { code: Code::KeyO, name: "O", vk: 0x4F },
    KeyMapping { code: Code::KeyP, name: "P", vk: 0x50 },
    KeyMapping { code: Code::KeyQ, name: "Q", vk: 0x51 },
    KeyMapping { code: Code::KeyR, name: "R", vk: 0x52 },
    KeyMapping { code: Code::KeyS, name: "S", vk: 0x53 },
    KeyMapping { code: Code::KeyT, name: "T", vk: 0x54 },
    KeyMapping { code: Code::KeyU, name: "U", vk: 0x55 },
    KeyMapping { code: Code::KeyV, name: "V", vk: 0x56 },
    KeyMapping { code: Code::KeyW, name: "W", vk: 0x57 },
    KeyMapping { code: Code::KeyX, name: "X", vk: 0x58 },
    KeyMapping { code: Code::KeyY, name: "Y", vk: 0x59 },
    KeyMapping { code: Code::KeyZ, name: "Z", vk: 0x5A },
    // Digits 0-9 (VK_0=0x30 .. VK_9=0x39)
    KeyMapping { code: Code::Digit0, name: "0", vk: 0x30 },
    KeyMapping { code: Code::Digit1, name: "1", vk: 0x31 },
    KeyMapping { code: Code::Digit2, name: "2", vk: 0x32 },
    KeyMapping { code: Code::Digit3, name: "3", vk: 0x33 },
    KeyMapping { code: Code::Digit4, name: "4", vk: 0x34 },
    KeyMapping { code: Code::Digit5, name: "5", vk: 0x35 },
    KeyMapping { code: Code::Digit6, name: "6", vk: 0x36 },
    KeyMapping { code: Code::Digit7, name: "7", vk: 0x37 },
    KeyMapping { code: Code::Digit8, name: "8", vk: 0x38 },
    KeyMapping { code: Code::Digit9, name: "9", vk: 0x39 },
    // Function keys F1-F12 (VK_F1=0x70 .. VK_F12=0x7B)
    KeyMapping { code: Code::F1,  name: "F1",  vk: 0x70 },
    KeyMapping { code: Code::F2,  name: "F2",  vk: 0x71 },
    KeyMapping { code: Code::F3,  name: "F3",  vk: 0x72 },
    KeyMapping { code: Code::F4,  name: "F4",  vk: 0x73 },
    KeyMapping { code: Code::F5,  name: "F5",  vk: 0x74 },
    KeyMapping { code: Code::F6,  name: "F6",  vk: 0x75 },
    KeyMapping { code: Code::F7,  name: "F7",  vk: 0x76 },
    KeyMapping { code: Code::F8,  name: "F8",  vk: 0x77 },
    KeyMapping { code: Code::F9,  name: "F9",  vk: 0x78 },
    KeyMapping { code: Code::F10, name: "F10", vk: 0x79 },
    KeyMapping { code: Code::F11, name: "F11", vk: 0x7A },
    KeyMapping { code: Code::F12, name: "F12", vk: 0x7B },
];

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
            code = text_to_code(&lower);
            code?;
        } else {
            return None;
        }
    }

    code.map(|c| (modifiers, c))
}

/// 将文本（已 lowercase）转换为 Code
pub fn text_to_code(s: &str) -> Option<Code> {
    // 查找统一映射表
    for entry in KEY_MAP {
        if s.eq_ignore_ascii_case(entry.name) {
            return Some(entry.code);
        }
    }
    None
}

/// 将 Windows 虚拟键码 (VK_*) 转换为 global_hotkey Code
pub fn vk_to_code(vk: VIRTUAL_KEY) -> Option<Code> {
    for entry in KEY_MAP {
        if entry.vk == vk.0 {
            return Some(entry.code);
        }
    }
    None
}

/// 将 (Modifiers, Code) 转换为人类可读的快捷键字符串，如 "Ctrl+Shift+D"
///
/// 不支持的 Code 返回空字符串
pub fn format_hotkey(modifiers: Modifiers, code: Code) -> String {
    let mut parts: Vec<&'static str> = Vec::new();

    if modifiers.contains(Modifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if modifiers.contains(Modifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(Modifiers::SHIFT) {
        parts.push("Shift");
    }
    if modifiers.contains(Modifiers::SUPER) {
        parts.push("Win");
    }

    if let Some(key) = format_code(code) {
        parts.push(key);
    } else {
        return String::new();
    }

    parts.join("+")
}

fn format_code(code: Code) -> Option<&'static str> {
    for entry in KEY_MAP {
        if entry.code == code {
            return Some(entry.name);
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

    #[test]
    fn test_format_ctrl_shift_d() {
        let s = format_hotkey(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyD);
        assert_eq!(s, "Ctrl+Shift+D");
    }

    /// Round-trip: text_to_code -> format_code should return the original text (uppercase)
    #[test]
    fn test_text_to_code_round_trip() {
        // Letters A-Z
        for c in 'A'..='Z' {
            let code = text_to_code(&c.to_lowercase().to_string()).unwrap_or_else(|| {
                panic!("text_to_code failed for '{}'", c)
            });
            let formatted = format_code(code).unwrap_or_else(|| {
                panic!("format_code failed for {:?}", code)
            });
            assert_eq!(formatted, &c.to_string(), "round-trip failed for '{}'", c);
        }
        // Digits 0-9
        for c in '0'..='9' {
            let code = text_to_code(&c.to_string()).unwrap_or_else(|| {
                panic!("text_to_code failed for '{}'", c)
            });
            let formatted = format_code(code).unwrap_or_else(|| {
                panic!("format_code failed for {:?}", code)
            });
            assert_eq!(formatted, &c.to_string(), "round-trip failed for '{}'", c);
        }
        // Function keys F1-F12
        for n in 1..=12 {
            let name = format!("F{}", n);
            let code = text_to_code(&name.to_lowercase()).unwrap_or_else(|| {
                panic!("text_to_code failed for '{}'", name)
            });
            let formatted = format_code(code).unwrap_or_else(|| {
                panic!("format_code failed for {:?}", code)
            });
            assert_eq!(formatted, &name, "round-trip failed for '{}'", name);
        }
    }

    /// vk_to_code -> format_code should produce consistent results
    #[test]
    fn test_vk_to_code_format_consistency() {
        // Letters: VK_A=0x41 .. VK_Z=0x5A
        for (offset, c) in ('A'..='Z').enumerate() {
            let vk = VIRTUAL_KEY(0x41 + offset as u16);
            let code = vk_to_code(vk).unwrap_or_else(|| {
                panic!("vk_to_code failed for VK_{}", c)
            });
            let formatted = format_code(code).unwrap_or_else(|| {
                panic!("format_code failed for {:?}", code)
            });
            assert_eq!(formatted, &c.to_string(), "vk round-trip failed for VK_{}", c);
        }
        // Digits: VK_0=0x30 .. VK_9=0x39
        for d in 0u8..=9 {
            let vk = VIRTUAL_KEY(0x30 + d as u16);
            let code = vk_to_code(vk).unwrap_or_else(|| {
                panic!("vk_to_code failed for VK_{}", d)
            });
            let formatted = format_code(code).unwrap_or_else(|| {
                panic!("format_code failed for {:?}", code)
            });
            assert_eq!(formatted, &d.to_string(), "vk round-trip failed for VK_{}", d);
        }
        // Function keys: VK_F1=0x70 .. VK_F12=0x7B
        for n in 1u8..=12 {
            let vk = VIRTUAL_KEY(0x70 + n as u16 - 1);
            let code = vk_to_code(vk).unwrap_or_else(|| {
                panic!("vk_to_code failed for VK_F{}", n)
            });
            let formatted = format_code(code).unwrap_or_else(|| {
                panic!("format_code failed for {:?}", code)
            });
            assert_eq!(formatted, &format!("F{}", n), "vk round-trip failed for VK_F{}", n);
        }
    }

    /// text_to_code should accept already-lowercase input without redundant allocation
    #[test]
    fn test_text_to_code_accepts_lowercase() {
        assert_eq!(text_to_code("a"), Some(Code::KeyA));
        assert_eq!(text_to_code("f1"), Some(Code::F1));
    }

    /// text_to_code should also accept uppercase input
    #[test]
    fn test_text_to_code_accepts_uppercase() {
        assert_eq!(text_to_code("A"), Some(Code::KeyA));
        assert_eq!(text_to_code("F1"), Some(Code::F1));
    }
}
