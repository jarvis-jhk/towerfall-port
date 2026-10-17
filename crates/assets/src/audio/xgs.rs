//! XACT global settings (`*.xgs`, content version 46): audio categories and
//! global variables with their names. RPC curves and DSP presets are only
//! counted.

use super::Reader;
use crate::error::{Error, Result, read_file};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub name: String,
    pub instance_limit: u8,
    pub fade_in_ms: u16,
    pub fade_out_ms: u16,
    pub instance_flags: u8,
    /// Index of the parent category, `None` for the root.
    pub parent: Option<u16>,
    /// Raw XACT volume byte.
    pub volume: u8,
    pub visibility: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub name: String,
    pub flags: u8,
    pub initial: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Debug)]
pub struct GlobalSettings {
    pub path: PathBuf,
    pub categories: Vec<Category>,
    pub variables: Vec<Variable>,
    pub rpc_count: u16,
    pub dsp_preset_count: u16,
}

impl GlobalSettings {
    pub fn load(path: &Path) -> Result<GlobalSettings> {
        let d = read_file(path)?;
        let short = || Error::format(path, "truncated global settings");
        if d.len() < 0x4d || &d[0..4] != b"XGSF" {
            return Err(Error::format(path, "not an XACT settings file (no XGSF)"));
        }
        let mut r = Reader::at(&d, 4);
        let version = r.u16().ok_or_else(short)?;
        if version != 46 {
            return Err(Error::format(
                path,
                format!("unsupported settings version {version}"),
            ));
        }
        r.pos = 0x13;
        let n_cat = r.u16().ok_or_else(short)? as usize;
        let n_var = r.u16().ok_or_else(short)? as usize;
        let _u1 = r.u16();
        let _u2 = r.u16();
        let rpc_count = r.u16().ok_or_else(short)?;
        let dsp_preset_count = r.u16().ok_or_else(short)?;
        let _dsp_params = r.u16();
        let cat_off = r.u32().ok_or_else(short)? as usize;
        let var_off = r.u32().ok_or_else(short)? as usize;
        let _u1o = r.u32();
        let _cat_name_idx = r.u32();
        let _u2o = r.u32();
        let _var_name_idx = r.u32();
        let cat_names = r.u32().ok_or_else(short)? as usize;
        let var_names = r.u32().ok_or_else(short)? as usize;
        let names = |start: usize, n: usize| -> Option<Vec<String>> {
            let mut nr = Reader::at(&d, start);
            (0..n).map(|_| nr.cstr()).collect()
        };
        let cn = names(cat_names, n_cat).ok_or_else(short)?;
        let vn = names(var_names, n_var).ok_or_else(short)?;
        let mut categories = Vec::new();
        let mut cr = Reader::at(&d, cat_off);
        for name in cn {
            let instance_limit = cr.u8().ok_or_else(short)?;
            let fade_in_ms = cr.u16().ok_or_else(short)?;
            let fade_out_ms = cr.u16().ok_or_else(short)?;
            let instance_flags = cr.u8().ok_or_else(short)?;
            let parent = cr.u16().ok_or_else(short)?;
            let volume = cr.u8().ok_or_else(short)?;
            let visibility = cr.u8().ok_or_else(short)?;
            categories.push(Category {
                name,
                instance_limit,
                fade_in_ms,
                fade_out_ms,
                instance_flags,
                parent: (parent != 0xffff).then_some(parent),
                volume,
                visibility,
            });
        }
        let mut variables = Vec::new();
        let mut vr = Reader::at(&d, var_off);
        for name in vn {
            variables.push(Variable {
                name,
                flags: vr.u8().ok_or_else(short)?,
                initial: vr.f32().ok_or_else(short)?,
                min: vr.f32().ok_or_else(short)?,
                max: vr.f32().ok_or_else(short)?,
            });
        }
        Ok(GlobalSettings {
            path: path.to_owned(),
            categories,
            variables,
            rpc_count,
            dsp_preset_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn music_settings() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let g =
            GlobalSettings::load(&steam.content.music_dir().join("TowerFallMusic.xgs")).unwrap();
        let names: Vec<_> = g.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Global", "Default", "Music"]);
        assert_eq!(g.variables.len(), 7);
        assert_eq!(g.variables[0].name, "NumCueInstances");
    }
}
