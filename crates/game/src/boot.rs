//! First run without game data: a plain screen that says what is needed and
//! where we looked, and starts the game as soon as data is found (a folder
//! dropped on the window, a Steam install appearing, a re-check every two
//! seconds). Drawn with the built-in debug font: no game art exists yet.

use std::path::PathBuf;

use anyhow::Result;
use tfp_render::glam::Vec2;
use tfp_render::{Color, SpriteFont, debug_font, debug_font_image};

use crate::app::{DrawContext, LoadContext, Scene, TickContext};
use crate::audio::Audio;
use crate::front::{FrontEnd, FrontOptions};
use crate::game::Game;

pub struct Boot {
    front: FrontOptions,
    keyboard: bool,
    persist: bool,
    game: Option<Game>,
    found: bool,
    font: Option<SpriteFont>,
    searched: Vec<PathBuf>,
    note: Option<(String, Color)>,
    /// `LoadContext` needs an audio handle; loading the menus does not use
    /// it (sounds start once the game ticks with the real one).
    quiet: Option<Audio>,
}

pub fn have_game_data() -> bool {
    let i = tfp_assets::Installs::discover();
    i.steam.is_some() || i.eight_player.is_some()
}

impl Boot {
    pub fn new(front: FrontOptions, keyboard: bool, persist: bool) -> Self {
        Self {
            front,
            keyboard,
            persist,
            game: None,
            found: false,
            font: None,
            searched: Vec::new(),
            note: None,
            quiet: None,
        }
    }

    fn start(&mut self, gpu: &tfp_render::Gpu, renderer: &mut tfp_render::Renderer) -> Result<()> {
        let mut g = Game::new(FrontEnd::new(self.front.clone()), self.keyboard);
        let audio = match &mut self.quiet {
            Some(a) => a,
            None => self.quiet.insert(Audio::offline(None)?),
        };
        g.load(&mut LoadContext {
            gpu,
            renderer,
            audio,
        })?;
        self.game = Some(g);
        Ok(())
    }

    /// A folder dropped on the window: accept it if it is a game install.
    fn dropped(&mut self, p: &std::path::Path) {
        let root = tfp_assets::install::normalize_game_dir(p);
        match tfp_assets::GameInstall::open(&root, None) {
            Some(inst) => {
                println!("game data: {}", inst.root.display());
                let mut o = self.front.initial.clone().unwrap_or_default();
                if !o.game_dirs.contains(&root) {
                    o.game_dirs.push(root);
                }
                tfp_assets::install::set_extra_dirs(o.game_dirs.clone());
                if self.persist
                    && let Some(path) = crate::options_store::path()
                    && let Err(e) = crate::options_store::save_to(&path, &o)
                {
                    log::warn!("saving options: {e:#}");
                }
                self.front.initial = Some(o);
                self.found = true;
            }
            None => {
                self.note = Some((
                    format!(
                        "NOT A TOWERFALL FOLDER (NO CONTENT/ATLAS INSIDE): {}",
                        p.display()
                    ),
                    Color::hex(0xFF5335),
                ));
            }
        }
    }
}

impl Scene for Boot {
    fn game_size(&self) -> (u32, u32) {
        self.game.as_ref().map_or((320, 240), |g| g.game_size())
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        if have_game_data() {
            let mut g = Game::new(FrontEnd::new(self.front.clone()), self.keyboard);
            g.load(ctx)?;
            self.game = Some(g);
            return Ok(());
        }
        println!("game data: not found; showing the help screen");
        let tex = ctx.renderer.create_texture(ctx.gpu, &debug_font_image());
        self.font = Some(debug_font(tex));
        self.searched = tfp_assets::install::searched_dirs();
        Ok(())
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        if let Some(g) = &mut self.game {
            g.tick(ctx);
            return;
        }
        for p in ctx.input.dropped.clone() {
            self.dropped(&p);
        }
        if !self.found && ctx.tick % 120 == 119 && have_game_data() {
            self.found = true;
        }
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        if self.found
            && self.game.is_none()
            && let Err(e) = self.start(ctx.gpu, ctx.renderer)
        {
            self.found = false;
            self.note = Some((
                format!("COULD NOT LOAD THE GAME DATA: {e:#}"),
                Color::hex(0xFF5335),
            ));
        }
        if let Some(g) = &mut self.game {
            g.draw(ctx);
            return;
        }
        let Some(f) = &self.font else { return };
        let dl = &mut *ctx.list;
        dl.rect(0.0, 0.0, 320.0, 240.0, Color::hex(0x1C1424));
        let gold = Color::hex(0xFFE23F);
        let white = Color::hex(0xE8E8E8);
        let grey = Color::hex(0x9A90A8);
        // (text, colour, scale, extra space before)
        let mut lines: Vec<(String, Color, f32, f32)> =
            vec![("TOWERFALL GAME DATA NOT FOUND".into(), gold, 2.0, 0.0)];
        for (k, s) in [
            "TOWERFALL-PORT IS A NEW ENGINE FOR TOWERFALL ASCENSION. IT NEEDS",
            "YOUR OWN COPY OF THE GAME (STEAM; WINDOWS, LINUX OR MAC VERSION)",
            "AND READS ITS ART, SOUND, MUSIC AND LEVELS FROM THERE AT RUN TIME.",
            "NOTHING IS COPIED OR CHANGED. OPTIONAL: THE DARK WORLD EXPANSION",
            "AND THE OFFICIAL 8-PLAYER BUILD (WIDE ARENAS FOR 5-12 ARCHERS).",
        ]
        .iter()
        .enumerate()
        {
            lines.push((s.to_string(), white, 1.0, if k == 0 { 4.0 } else { 0.0 }));
        }
        lines.push(("TO FIX IT, ANY OF:".into(), gold, 1.0, 6.0));
        for s in [
            "- INSTALL TOWERFALL ASCENSION WITH STEAM (IT IS FOUND BY ITSELF)",
            "- DRAG THE GAME FOLDER (THE ONE WITH CONTENT/ INSIDE) ONTO THIS",
            "  WINDOW; IT IS REMEMBERED",
            "- START WITH --GAME-DIR PATH, OR SET TF_GAME_DIR (TF8_GAME_DIR",
            "  FOR THE 8-PLAYER BUILD)",
        ] {
            lines.push((s.to_string(), white, 1.0, 0.0));
        }
        lines.push(("LOOKED IN:".into(), gold, 1.0, 6.0));
        if tfp_assets::install::steam_library_dirs().is_empty() {
            lines.push((
                "(NO STEAM LIBRARY: ~/.steam, ~/.local/share/Steam, FLATPAK, REGISTRY)".into(),
                grey,
                1.0,
                0.0,
            ));
        }
        for p in self.searched.iter().take(10) {
            let s = p.display().to_string();
            let s = if s.chars().count() > 74 {
                let skip = s.chars().count() - 71;
                format!("...{}", s.chars().skip(skip).collect::<String>())
            } else {
                s
            };
            lines.push((s, grey, 1.0, 0.0));
        }
        if self.searched.len() > 10 {
            lines.push((
                format!("(+{} MORE)", self.searched.len() - 10),
                grey,
                1.0,
                0.0,
            ));
        }
        let mut y = 10.0;
        for (s, c, scale, gap) in &lines {
            y += gap;
            f.draw(dl, s, Vec2::new(8.0, y), *c, *scale);
            y += 7.0 * scale;
        }
        if let Some((n, c)) = &self.note {
            f.draw(dl, n, Vec2::new(8.0, 226.0), *c, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_a_game_folder_is_accepted() {
        let mut b = Boot::new(FrontOptions::default(), false, false);
        b.dropped(std::path::Path::new("/nonexistent/folder"));
        assert!(!b.found && b.note.is_some());
        let Some(inst) = tfp_assets::Installs::discover().steam else {
            return;
        };
        // The Content folder itself is accepted too.
        b.dropped(&inst.root.join("Content"));
        assert!(b.found);
        let dirs = b.front.initial.as_ref().unwrap().game_dirs.clone();
        assert_eq!(dirs, vec![inst.root.clone()]);
        tfp_assets::install::set_extra_dirs(Vec::new());
    }
}
