fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.set("ProductName", "RigStats");
        res.set("FileDescription", "RigStats Hardware Monitor");
        res.set("InternalName", "rigstats");
        res.compile().expect("failed to embed Windows resources");
    }
}
