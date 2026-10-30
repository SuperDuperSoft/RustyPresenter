use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::Path};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slide {
    pub id: Uuid,
    pub label: String,
    pub text: String,
}

impl Slide {
    pub fn new(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self { id: Uuid::new_v4(), label: label.into(), text: text.into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceItem {
    pub id: Uuid,
    pub title: String,
    pub kind: String,
    pub credits: String,
    pub slides: Vec<Slide>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub version: u32,
    pub title: String,
    pub theme: String,
    pub ccli_license: String,
    pub items: Vec<ServiceItem>,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            version: 1,
            title: "Sunday service".into(),
            theme: "midnight".into(),
            ccli_license: String::new(),
            items: vec![ServiceItem {
                id: Uuid::new_v4(), title: "Welcome".into(), kind: "announcement".into(),
                credits: String::new(),
                slides: vec![Slide::new("Welcome", "Welcome\nWe're glad you're here")],
            }],
        }
    }
}

impl Service {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 { return Err("This service file uses an unsupported version.".into()); }
        if self.title.trim().is_empty() || self.title.chars().count() > 200 {
            return Err("Give the service a title of 1–200 characters.".into());
        }
        if !["midnight", "ocean", "forest", "plain"].contains(&self.theme.as_str()) {
            return Err("Choose a supported presentation theme.".into());
        }
        if self.items.len() > 500 || self.ccli_license.len() > 100 {
            return Err("The service is too large.".into());
        }
        let mut ids = HashSet::new();
        for item in &self.items {
            if !ids.insert(item.id) || item.title.trim().is_empty() || item.title.len() > 500
                || item.credits.len() > 4000 || item.slides.is_empty() || item.slides.len() > 500
                || !["song", "scripture", "announcement"].contains(&item.kind.as_str()) {
                return Err("A service item is invalid or has duplicate IDs.".into());
            }
            for slide in &item.slides {
                if !ids.insert(slide.id) || slide.label.len() > 500 || slide.text.len() > 16000 {
                    return Err("A slide is too large or has a duplicate ID.".into());
                }
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() { return Ok(Self::default()); }
        let bytes = fs::read(path).map_err(|e| format!("Cannot read service: {e}"))?;
        let service: Self = serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read service JSON: {e}"))?;
        service.validate()?;
        Ok(service)
    }

    // Write completely before replacing the last good service file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = fs::File::create(&temporary).map_err(|e| format!("Cannot save service: {e}"))?;
        file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|e| format!("Cannot save service: {e}"))?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| format!("Cannot replace service file: {e}"))
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Live {
    pub slide: Option<Slide>,
    pub item_id: Option<Uuid>,
    pub item_title: String,
    pub credits: String,
    pub theme: String,
    pub blackout: bool,
    pub clear_text: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub service_revision: u64,
    pub service: Service,
    pub live: Live,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Take { item_id: Uuid, slide_id: Uuid },
    Next,
    Previous,
    Blackout { enabled: bool },
    ClearText { enabled: bool },
}

impl Snapshot {
    pub fn new(service: Service) -> Self {
        Self { revision: 0, service_revision: 0, service, live: Live::default() }
    }

    fn take(&mut self, item_id: Uuid, slide_id: Uuid) -> Result<(), String> {
        let item = self.service.items.iter().find(|i| i.id == item_id).ok_or("That item is no longer in the service.")?;
        let slide = item.slides.iter().find(|s| s.id == slide_id).ok_or("That slide is no longer in the service.")?;
        // On-air content is a snapshot: edits/deletions never change the audience screen.
        self.live.slide = Some(slide.clone());
        self.live.item_id = Some(item.id);
        self.live.item_title = item.title.clone();
        self.live.theme = self.service.theme.clone();
        self.live.credits = item.credits.clone();
        if item.kind == "song" && !self.service.ccli_license.trim().is_empty() {
            self.live.credits.push_str(&format!("\nCCLI License #{}", self.service.ccli_license.trim()));
        }
        Ok(())
    }

    pub fn command(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::Take { item_id, slide_id } => self.take(item_id, slide_id)?,
            Command::Blackout { enabled } => self.live.blackout = enabled,
            Command::ClearText { enabled } => self.live.clear_text = enabled,
            Command::Next | Command::Previous => {
                let cues: Vec<_> = self.service.items.iter().flat_map(|i| i.slides.iter().map(move |s| (i.id, s.id))).collect();
                if cues.is_empty() { return Err("Add slides to the service first.".into()); }
                let current = self.live.slide.as_ref().and_then(|slide| cues.iter().position(|c| Some(c.0) == self.live.item_id && c.1 == slide.id));
                let index = match (current, command) {
                    (Some(i), Command::Next) => (i + 1).min(cues.len() - 1),
                    (Some(i), _) => i.saturating_sub(1),
                    (None, _) => 0,
                };
                self.take(cues[index].0, cues[index].1)?;
            }
        }
        self.revision += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_and_removing_a_live_slide_does_not_change_the_output() {
        let mut state = Snapshot::new(Service::default());
        let item = &state.service.items[0];
        state.command(Command::Take { item_id: item.id, slide_id: item.slides[0].id }).unwrap();
        let original = state.live.slide.as_ref().unwrap().text.clone();
        state.service.items[0].slides[0].text = "Edited offline".into();
        state.service.items.clear();
        assert_eq!(state.live.slide.unwrap().text, original);
    }

    #[test]
    fn navigation_stays_in_bounds_and_preserves_blackout() {
        let mut state = Snapshot::new(Service::default());
        state.service.items[0].slides.push(Slide::new("Two", "Second slide"));
        state.command(Command::Blackout { enabled: true }).unwrap();
        state.command(Command::Next).unwrap();
        state.command(Command::Previous).unwrap();
        assert_eq!(state.live.slide.as_ref().unwrap().label, "Welcome");
        state.command(Command::Next).unwrap();
        state.command(Command::Next).unwrap();
        assert_eq!(state.live.slide.as_ref().unwrap().label, "Two");
        assert!(state.live.blackout);
    }

    #[test]
    fn invalid_import_cannot_overwrite_saved_service() {
        let dir = std::env::temp_dir().join(format!("rustypresenter-test-{}", Uuid::new_v4()));
        let path = dir.join("service.json");
        let mut service = Service::default();
        service.save(&path).unwrap();
        service.version = 999;
        assert!(service.save(&path).is_err());
        assert_eq!(Service::load(&path).unwrap().version, 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
