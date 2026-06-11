use egui::{Context, TextureHandle, TextureOptions};

#[derive(Clone)]
pub struct Textures {
    pub rog: TextureHandle,
    pub amd: TextureHandle,
    pub nvidia: TextureHandle,
    pub intel: TextureHandle,
    pub msi: TextureHandle,
    pub alienware: TextureHandle,
    pub razer: TextureHandle,
    pub legion: TextureHandle,
    pub omen: TextureHandle,
    pub aorus: TextureHandle,
    pub gigabyte: TextureHandle,
    pub predator: TextureHandle,
    pub taurus: TextureHandle,
}

impl Textures {
    pub fn load(ctx: &Context) -> Self {
        Self {
            rog: load_png(ctx, "rog", include_bytes!("../assets/ROG_logo_red.png")),
            amd: load_png(
                ctx,
                "amd",
                include_bytes!("../assets/AMD-Radeon-Ryzen-Symbol.png"),
            ),
            nvidia: load_png(ctx, "nvidia", include_bytes!("../assets/nvidia.png")),
            intel: load_png(ctx, "intel", include_bytes!("../assets/intel.png")),
            msi: load_png(ctx, "msi", include_bytes!("../assets/msi.png")),
            alienware: load_png(ctx, "alienware", include_bytes!("../assets/Alienware.png")),
            razer: load_png(ctx, "razer", include_bytes!("../assets/Razer.png")),
            legion: load_png(ctx, "legion", include_bytes!("../assets/Lenovo-Legion.png")),
            omen: load_png(ctx, "omen", include_bytes!("../assets/HP-Omen.png")),
            aorus: load_png(ctx, "aorus", include_bytes!("../assets/AORUS-Gigabyte.png")),
            gigabyte: load_png(ctx, "gigabyte", include_bytes!("../assets/gigabyte.png")),
            predator: load_png(
                ctx,
                "predator",
                include_bytes!("../assets/Acer-Predator.png"),
            ),
            taurus: load_png(ctx, "taurus", include_bytes!("../assets/taurus.png")),
        }
    }

    /// Brand logo for the rig header. Returns None when no image exists for this brand.
    pub fn rig_logo(&self, brand: &str) -> Option<&TextureHandle> {
        match brand {
            "rog" | "asus-rog" => Some(&self.rog),
            "msi" => Some(&self.msi),
            "alienware" => Some(&self.alienware),
            "razer" => Some(&self.razer),
            "legion" => Some(&self.legion),
            "omen" => Some(&self.omen),
            "aorus" => Some(&self.aorus),
            "gigabyte" => Some(&self.gigabyte),
            "predator" => Some(&self.predator),
            "taurus" => Some(&self.taurus),
            _ => None,
        }
    }

    /// CPU vendor logo derived from cpu_model string.
    pub fn cpu_logo(&self, cpu_model: &str) -> Option<&TextureHandle> {
        let m = cpu_model.to_ascii_lowercase();
        if m.contains("amd") || m.contains("ryzen") || m.contains("athlon") || m.contains("epyc") {
            Some(&self.amd)
        } else if m.contains("intel") || m.contains("core i") || m.contains("xeon") {
            Some(&self.intel)
        } else {
            None
        }
    }

    /// GPU vendor logo derived from gpu_name string.
    pub fn gpu_logo(&self, gpu_name: &str) -> Option<&TextureHandle> {
        let m = gpu_name.to_ascii_lowercase();
        if m.contains("amd") || m.contains("radeon") || m.contains("rx ") || m.contains("vega") {
            Some(&self.amd)
        } else if m.contains("nvidia")
            || m.contains("geforce")
            || m.contains("rtx")
            || m.contains("gtx")
        {
            Some(&self.nvidia)
        } else if m.contains("intel")
            || m.contains("arc")
            || m.contains("uhd")
            || m.contains("iris")
        {
            Some(&self.intel)
        } else {
            None
        }
    }
}

fn load_png(ctx: &Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let img = image::load_from_memory(bytes).expect("logo png").to_rgba8();
    let (w, h) = img.dimensions();
    let color_image =
        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    ctx.load_texture(name, color_image, TextureOptions::LINEAR)
}
