pub const KEY_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub const SCALE_NAMES: [&str; 9] = [
    "N/A",
    "Major",
    "Minor",
    "Penta",
    "Dorian",
    "Mixo",
    "Whole",
    "Hirajoshi",
    "Lydian",
];

pub const BANK_NAMES: [&str; 4] = ["A", "B", "C", "D"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Select,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoStatus {
    Off,
    On,
    NoHdmi,
}

impl VideoStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::NoHdmi => "No HDMI",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub key_index: usize,
    pub octave: i32,
    pub fine_tune_cents: f32,
    pub stereo_spread: u8,
    pub scale_index: usize,
    pub bank_index: usize,
    pub wt_volume: u8,
    pub gr_volume: u8,
    pub oscillators_active: bool,
    pub granular_active: bool,
    pub osc_count: usize,
    pub gr_voices: usize,
    pub video_status: VideoStatus,
    pub glide_progress: Option<f32>,
    pub selected_item: usize,
    pub scroll_offset: usize,
}

impl MenuState {
    pub fn new(fine_tune_cents: f32, osc_count: usize, gr_voices: usize) -> Self {
        Self {
            key_index: 9,
            octave: 1,
            fine_tune_cents,
            stereo_spread: 100,
            scale_index: 7,
            bank_index: 0,
            wt_volume: 50,
            gr_volume: 50,
            oscillators_active: false,
            granular_active: false,
            osc_count,
            gr_voices,
            video_status: VideoStatus::Off,
            glide_progress: None,
            selected_item: 0,
            scroll_offset: 0,
        }
    }

    pub fn key_name(&self) -> &'static str {
        KEY_NAMES[self.key_index]
    }

    pub fn total_items(&self) -> usize {
        14
    }

    pub fn apply_button(&mut self, button: Button) {
        const VISIBLE_ROWS: usize = 11;

        match button {
            Button::Up => {
                if self.selected_item == 0 {
                    self.selected_item = self.total_items().saturating_sub(1);
                } else {
                    self.selected_item -= 1;
                }
            }
            Button::Down => {
                self.selected_item = (self.selected_item + 1) % self.total_items();
            }
            Button::Select => self.increment_selected_value(),
            Button::Back => self.decrement_selected_value(),
        }

        // Adjust scroll offset to keep selected_item visible
        if self.selected_item < self.scroll_offset {
            self.scroll_offset = self.selected_item;
        } else if self.selected_item >= self.scroll_offset + VISIBLE_ROWS {
            self.scroll_offset = self.selected_item + 1 - VISIBLE_ROWS;
        }
    }

    fn increment_selected_value(&mut self) {
        match self.selected_item {
            0 => self.oscillators_active = !self.oscillators_active,
            1 => self.granular_active = !self.granular_active,
            2 => self.key_index = (self.key_index + 1) % KEY_NAMES.len(),
            3 => self.scale_index = (self.scale_index + 1) % SCALE_NAMES.len(),
            4 => self.octave = (self.octave + 1).min(8),
            5 => self.stereo_spread = (self.stereo_spread + 5).min(100),
            6 => self.bank_index = (self.bank_index + 1) % BANK_NAMES.len(),
            7 => self.wt_volume = (self.wt_volume + 10).min(100),
            8 => self.gr_volume = (self.gr_volume + 10).min(100),
            9 => self.fine_tune_cents = (self.fine_tune_cents + 1.0).min(100.0),
            10 => self.osc_count = (self.osc_count + 1).min(64),
            11 => self.gr_voices = (self.gr_voices + 1).min(64),
            _ => {}
        }
    }

    fn decrement_selected_value(&mut self) {
        match self.selected_item {
            0 => self.oscillators_active = !self.oscillators_active,
            1 => self.granular_active = !self.granular_active,
            2 => {
                if self.key_index == 0 {
                    self.key_index = KEY_NAMES.len() - 1;
                } else {
                    self.key_index -= 1;
                }
            }
            3 => {
                if self.scale_index == 0 {
                    self.scale_index = SCALE_NAMES.len() - 1;
                } else {
                    self.scale_index -= 1;
                }
            }
            4 => self.octave = (self.octave - 1).max(0),
            5 => self.stereo_spread = self.stereo_spread.saturating_sub(5),
            6 => {
                if self.bank_index == 0 {
                    self.bank_index = BANK_NAMES.len() - 1;
                } else {
                    self.bank_index -= 1;
                }
            }
            7 => self.wt_volume = self.wt_volume.saturating_sub(10),
            8 => self.gr_volume = self.gr_volume.saturating_sub(10),
            9 => self.fine_tune_cents = (self.fine_tune_cents - 1.0).max(-100.0),
            10 => self.osc_count = self.osc_count.saturating_sub(1).max(1),
            11 => self.gr_voices = self.gr_voices.saturating_sub(1),
            _ => {}
        }
    }

    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("Wavetable: {}", if self.oscillators_active { "On" } else { "Off" }),
            format!("Granular: {}", if self.granular_active { "On" } else { "Off" }),
            format!("Key: {}", self.key_name()),
            format!("Scale: {}", SCALE_NAMES[self.scale_index]),
            format!("Octave: {}", self.octave),
            format!("Stereo: {}", self.stereo_spread),
            format!("WT Bank: {}", BANK_NAMES[self.bank_index]),
            format!("WT Vol: {}", self.wt_volume),
            format!("GR Vol: {}", self.gr_volume),
            format!("Cents: {:+}", self.fine_tune_cents as i32),
            format!("WT Oscs: {}", self.osc_count),
            format!("GR Voices: {}", self.gr_voices),
            format!("Video: {}", self.video_status.as_str()),
            format!("Glide: {}", match self.glide_progress {
                None => "---".to_string(),
                Some(p) => format!("{}%", (p * 100.0) as u32),
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_navigation_wraps() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.apply_button(Button::Up);
        assert_eq!(menu.selected_item, menu.total_items() - 1);
    }

    #[test]
    fn menu_has_fourteen_items() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.total_items(), 14);
        assert_eq!(menu.lines().len(), 14);
    }

    #[test]
    fn menu_scroll_offset_initialized_to_zero() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn menu_scroll_shifts_beyond_eleven_rows() {
        // 14 items with 11 visible rows; scrolling past item 10 should increase scroll_offset
        let mut menu = MenuState::new(0.0, 8, 8);
        for _ in 0..11 {
            menu.apply_button(Button::Down);
        }
        assert_eq!(menu.selected_item, 11);
        assert!(menu.scroll_offset > 0, "scroll_offset should shift when selected item exceeds visible window");
    }

    #[test]
    fn menu_default_values_match_spec() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.key_index, 9);           // A
        assert_eq!(menu.octave, 1);
        assert_eq!(menu.scale_index, 7);         // HIRAJOSHI
        assert_eq!(menu.stereo_spread, 100);
        assert_eq!(menu.wt_volume, 50);
        assert_eq!(menu.gr_volume, 50);
        assert_eq!(menu.oscillators_active, false);
        assert_eq!(menu.granular_active, false);
        assert_eq!(menu.fine_tune_cents, 0.0);
    }

    #[test]
    fn menu_lines_correct_labels() {
        let menu = MenuState::new(0.0, 8, 8);
        let lines = menu.lines();
        assert!(lines[0].starts_with("Wavetable:"), "line 0 should start with Wavetable:");
        assert!(lines[1].starts_with("Granular:"), "line 1 should start with Granular:");
        assert!(lines[2].starts_with("Key:"), "line 2 should start with Key:");
        assert!(lines[3].starts_with("Scale:"), "line 3 should start with Scale:");
        assert!(lines[4].starts_with("Octave:"), "line 4 should start with Octave:");
        assert!(lines[5].starts_with("Stereo:"), "line 5 should start with Stereo:");
        assert!(lines[6].starts_with("WT Bank:"), "line 6 should start with WT Bank:");
        assert!(lines[7].starts_with("WT Vol:"), "line 7 should start with WT Vol:");
        assert!(lines[8].starts_with("GR Vol:"), "line 8 should start with GR Vol:");
        assert!(lines[9].starts_with("Cents:"), "line 9 should start with Cents:");
        assert!(lines[10].starts_with("WT Oscs:"), "line 10 should start with WT Oscs:");
        assert!(lines[11].starts_with("GR Voices:"), "line 11 should start with GR Voices:");
        assert!(lines[12].starts_with("Video:"), "line 12 should start with Video:");
    }

    #[test]
    fn menu_video_line_reports_state() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.lines()[12], "Video: Off");
        menu.video_status = VideoStatus::On;
        assert_eq!(menu.lines()[12], "Video: On");
    }

    #[test]
    fn menu_glide_item_none_shows_dashes() {
        let mut menu = MenuState::new(0.0, 4, 2);
        menu.glide_progress = None;
        let lines = menu.lines();
        assert!(lines.iter().any(|l| l.contains("Glide: ---")));
    }

    #[test]
    fn menu_glide_item_some_shows_percent() {
        let mut menu = MenuState::new(0.0, 4, 2);
        menu.glide_progress = Some(0.5);
        let lines = menu.lines();
        assert!(lines.iter().any(|l| l.contains("Glide: 50%")));
    }

    #[test]
    fn glide_item_is_read_only() {
        let mut menu = MenuState::new(0.0, 4, 2);
        menu.selected_item = 13; // GLIDE item
        let before = menu.lines();
        menu.apply_button(Button::Select);
        menu.apply_button(Button::Back);
        assert_eq!(menu.lines(), before, "GLIDE item should be read-only");
    }

    #[test]
    fn menu_total_items_is_14() {
        let mut menu = MenuState::new(0.0, 4, 2);
        assert_eq!(menu.total_items(), 14);
        menu.video_status = VideoStatus::NoHdmi;
        assert_eq!(menu.lines()[12], "Video: No HDMI");
    }

    #[test]
    fn granular_toggle_activates() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.granular_active, false);
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        assert_eq!(menu.granular_active, true);
        menu.apply_button(Button::Select);
        assert_eq!(menu.granular_active, false);
    }

    #[test]
    fn wavetable_toggle_activates() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.oscillators_active, false);
        menu.selected_item = 0;
        menu.apply_button(Button::Select);
        assert_eq!(menu.oscillators_active, true);
        menu.apply_button(Button::Back);
        assert_eq!(menu.oscillators_active, false);
    }

    #[test]
    fn wt_volume_increments_by_ten() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.wt_volume, 50);
        menu.selected_item = 7;
        menu.apply_button(Button::Select);
        assert_eq!(menu.wt_volume, 60);
        menu.apply_button(Button::Back);
        assert_eq!(menu.wt_volume, 50);
    }

    #[test]
    fn gr_volume_increments_by_ten() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.gr_volume, 50);
        menu.selected_item = 8;
        menu.apply_button(Button::Select);
        assert_eq!(menu.gr_volume, 60);
        menu.apply_button(Button::Back);
        assert_eq!(menu.gr_volume, 50);
    }

    #[test]
    fn osc_count_increments_by_one_clamped() {
        let mut menu = MenuState::new(0.0, 64, 8);
        menu.selected_item = 10;
        menu.apply_button(Button::Select);
        assert_eq!(menu.osc_count, 64, "osc_count should clamp at 64");
        let mut menu2 = MenuState::new(0.0, 1, 8);
        menu2.selected_item = 10;
        menu2.apply_button(Button::Back);
        assert_eq!(menu2.osc_count, 1, "osc_count should clamp at minimum 1");
    }

    #[test]
    fn gr_voices_increments_by_one_clamped() {
        let mut menu = MenuState::new(0.0, 8, 64);
        menu.selected_item = 11;
        menu.apply_button(Button::Select);
        assert_eq!(menu.gr_voices, 64, "gr_voices should clamp at 64");
        let mut menu2 = MenuState::new(0.0, 8, 0);
        menu2.selected_item = 11;
        menu2.apply_button(Button::Back);
        assert_eq!(menu2.gr_voices, 0, "gr_voices should allow zero");
    }
}
