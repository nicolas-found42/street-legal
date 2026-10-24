//! Tier-D static OSD button projection. Native437320 loads the CFG's render,
//! body and name;4373e0 consumes a six-component CSV transform. Original TUFA
//! Button/Group code retains commands and interaction decisions.
use crate::factory::CfgBinding;
use std::{cell::RefCell, collections::HashMap};
#[derive(Clone)]
pub struct Button {
    pub owner: u32,
    pub render: u32,
    pub body: u32,
    pub transform: [f32; 6],
    pub name: String,
}
struct Template {
    render: CfgBinding,
    body: CfgBinding,
    name: String,
}
thread_local! {static TEMPLATES:RefCell<HashMap<u32,Template>>=RefCell::new(HashMap::new());static BUTTONS:RefCell<HashMap<u32,Button>>=RefCell::new(HashMap::new());}
pub fn clear() {
    TEMPLATES.with(|v| v.borrow_mut().clear());
    BUTTONS.with(|v| v.borrow_mut().clear());
}
pub fn load(id: u32, path: &str) -> Result<(), String> {
    if TEMPLATES.with(|v| v.borrow().contains_key(&id)) {
        return Ok(());
    }
    let bytes = crate::factory::read_cfg_file(path)?;
    let package = crate::factory::package_for_resource(id)?;
    let mut template = Template {
        render: CfgBinding::default(),
        body: CfgBinding::default(),
        name: String::new(),
    };
    for line in String::from_utf8_lossy(&bytes).lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let key = words.next().unwrap();
        match key {
            "eof" => break,
            "render" | "body" => {
                let value = words.next().ok_or("button CFG missing resource")?;
                let local = u32::from_str_radix(value.trim_start_matches("0x"), 16)
                    .map_err(|e| e.to_string())?;
                let canonical = crate::factory::lookup_cfg(&package, local)?;
                let binding = crate::factory::bind_cfg(
                    id,
                    canonical,
                    if key == "render" { 0 } else { 9 },
                    0,
                )?;
                let valid_kind = slr_resource::node::with_res(|h| {
                    h.container(binding.container).is_some_and(|c| {
                        if key == "render" {
                            matches!(c.type_id, 3 | 14)
                        } else {
                            c.type_id == 9
                        }
                    })
                });
                if !valid_kind {
                    return Err(format!("button {key} resource has wrong kind"));
                }
                if key == "render" {
                    template.render = binding;
                } else {
                    template.body = binding;
                }
            }
            "name" => template.name = words.next().ok_or("button CFG missing name")?.into(),
            _ => return Err(format!("NativeButtonBoundary: unsupported CFG {line}")),
        }
    }
    TEMPLATES.with(|v| v.borrow_mut().insert(id, template));
    Ok(())
}
pub fn created(id: u32, container: u32, owner: u32, parameters: &str) -> Result<(), String> {
    let mut transform = [0.; 6];
    if !parameters.is_empty() {
        let parsed = slr_fmt_cfg::scan::sscanf(
            parameters.as_bytes(),
            b"%f,%f,%f,%f,%f,%f",
            slr_fmt_cfg::Precision::F32,
        );
        if parsed.f(2).is_none() {
            return Err(format!(
                "NativeButtonBoundary: invalid transform {parameters:?}"
            ));
        }
        for (i, out) in transform.iter_mut().enumerate() {
            *out = parsed.f(i).unwrap_or(0.);
        }
    }
    if transform[3..].iter().any(|v| *v != 0.) {
        return Err("NativeButtonBoundary: rotated OSD projection is unsupported".into());
    }
    let button = TEMPLATES.with(|v| {
        let templates = v.borrow();
        let t = templates.get(&id).ok_or("button template unavailable")?;
        Ok::<_, String>(Button {
            owner,
            render: t.render.id,
            body: t.body.id,
            transform,
            name: t.name.clone(),
        })
    })?;
    slr_telemetry::journal::emit(
        "native-button",
        serde_json::json!({"container":container,"template":id,"owner":owner,"render":button.render,"body":button.body,"transform":transform,"name":button.name,"projection":"tier-d-osd"}),
    )?;
    BUTTONS.with(|v| v.borrow_mut().insert(container, button));
    Ok(())
}
pub fn buttons() -> Vec<(u32, Button)> {
    let mut buttons: Vec<_> =
        BUTTONS.with(|v| v.borrow().iter().map(|(id, b)| (*id, b.clone())).collect());
    buttons.sort_by_key(|(id, _)| *id);
    buttons
}
