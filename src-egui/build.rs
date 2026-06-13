fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.set("ProductName", "RIGStats");
        res.set("FileDescription", "RIGStats");
        res.set("CompanyName", "Code By AB");
        res.set("InternalName", "rigstats");
        res.compile().expect("failed to embed Windows resources");
    }
}
