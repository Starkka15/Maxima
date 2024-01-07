pub fn html_to_easymark(html: &str) -> String {
    let mut rtn = html
        .to_string()
        .replace("\n", "")
        .replace("*", "\\*")
        .replace("<br>", "\n")
        .replace("<b>", "*")
        .replace("</b>", "*")
        .replace("<span style=\"font-size: small;\">", "") // WE DO NOT CARE 🗣️
        .replace("</span>", "")
        .replace("&nbsp;", " ")
        .replace("\\", "");
    let mut idx: usize = 0;

    // link corrector
    while rtn.len() > idx
        && rtn[idx..].find("<a href=\"").is_some()
        && rtn[idx..].find("\">").is_some()
    {
        let segment = rtn[idx..].to_string();

        let beglink = segment.find("<a href=\"").expect("welp") + 9;
        let endlink = segment[beglink..].find("\"").expect("welp") + beglink;
        let begtext = segment[endlink..].find("\">").expect("welp") + endlink + 2;
        let endtext = segment[begtext..].find("</a>").expect("welp") + begtext;

        let modlink: String = "[".to_string()
            + &segment[begtext..endtext].to_string()
            + "]("
            + &segment[beglink..endlink].to_string()
            + ")";
        rtn = rtn[..idx + (beglink - 9)].to_string()
            + &modlink
            + &rtn[idx + endtext + 4..].to_string();

        idx += (beglink - 9) + (modlink.len());
    }
    idx = 0;

    // image removal, easymark doesn't support them and EAD doesn't seem to display them
    while rtn.len() > idx && rtn[idx..].find("<img").is_some() && rtn[idx..].find(">").is_some() {
        let segment = rtn[idx..].to_string();

        let begimg = segment.find("<img").expect("welp") + 4;
        let endimg = segment[begimg..].find(">").expect("welp") + begimg;

        rtn = rtn[..idx + (begimg - 4)].to_string() + &rtn[idx + endimg + 1..].to_string();

        idx += begimg - 4;
    }

    rtn.replace("/", "\\/") // links are caught in the crossfire of this, but clicking them still leads to where they should
        .replace("<i>", "/")
        .replace("<\\/i>", "/")
}
