use crate::catalog::{Result, validate_rating};
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

pub fn read(xml: &str) -> Result<Option<i32>> {
    if xml.len() > 1_048_576 {
        return Err("XMP 文件过大".into());
    }
    let doc = roxmltree::Document::parse(xml).map_err(|_| "XMP 格式无效")?;
    let mut values = Vec::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        for a in n
            .attributes()
            .filter(|a| a.namespace() == Some(XMP) && a.name() == "Rating")
        {
            values.push(a.value());
        }
        if n.has_tag_name((XMP, "Rating")) {
            if n.children().any(|c| c.is_element()) {
                return Err("XMP 评分含不支持的嵌套结构".into());
            }
            values.push(n.text().unwrap_or("0"));
        }
    }
    if values.len() > 1 {
        return Err("XMP 含重复评分，需先解决冲突".into());
    }
    let value = values.first().copied().unwrap_or("0").trim();
    if value.is_empty() {
        return Ok(None);
    }
    // xmp:Rating is Real. Accept integer-valued decimals without rounding fractions.
    let integer = if let Some((whole, fraction)) = value.split_once('.') {
        if !fraction.bytes().all(|b| b == b'0') {
            return Err("XMP 含非整数评分，暂不自动转换".into());
        }
        whole
    } else {
        value
    };
    let rating = integer
        .parse::<i32>()
        .map_err(|_| "XMP 评分无效或暂不支持此数值写法")?;
    match rating {
        -1 => Ok(Some(0)),
        0 => Ok(None),
        r => {
            validate_rating(Some(r))?;
            Ok(Some(r))
        }
    }
}

// DOM source ranges preserve every unrelated byte. New packets use the standard namespace.
pub fn merge(xml: Option<&str>, rating: Option<i32>) -> Result<String> {
    validate_rating(rating)?;
    let value = match rating {
        None => 0,
        Some(0) => -1,
        Some(r) => r,
    }
    .to_string();
    let Some(xml) = xml else {
        return Ok(format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\"><rdf:Description xmlns:xmp=\"{XMP}\" xmp:Rating=\"{value}\"/></rdf:RDF></x:xmpmeta>"
        ));
    };
    read(xml)?;
    let doc = roxmltree::Document::parse(xml).map_err(|_| "XMP 格式无效")?;
    for node in doc.descendants().filter(|n| n.is_element()) {
        if let Some(attr) = node
            .attributes()
            .find(|a| a.namespace() == Some(XMP) && a.name() == "Rating")
        {
            let mut output = xml.to_string();
            output.replace_range(attr.range_value(), &value);
            return Ok(output);
        }
        if node.has_tag_name((XMP, "Rating")) {
            let text: Vec<_> = node.children().filter(|n| n.is_text()).collect();
            if text.len() != 1 {
                return Err("保留现有 XMP：评分结构不支持安全改写".into());
            }
            let mut output = xml.to_string();
            output.replace_range(text[0].range(), &value);
            return Ok(output);
        }
    }
    // Inserting a sibling description avoids reparsing/reformatting unknown metadata.
    let rdf = doc
        .descendants()
        .find(|n| n.has_tag_name((RDF, "RDF")))
        .ok_or("未找到 RDF 元数据")?;
    let range = rdf.range();
    let fragment = &xml[range.clone()];
    if fragment.trim_end().ends_with("/>") {
        return Err("暂不改写空 RDF 包，请保留现有文件".into());
    }
    let end = range.start + fragment.rfind("</").ok_or("RDF 包缺少结束标记")?;
    let mut output = xml.to_string();
    output.insert_str(
        end,
        &format!(
            "<rdf:Description xmlns:rdf=\"{RDF}\" xmlns:xmp=\"{XMP}\" xmp:Rating=\"{value}\"/>"
        ),
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer_valued_real_ratings_are_supported_without_rounding_fractions() {
        for (text, rating) in [("4.000", Some(4)), ("-1.0", Some(0)), ("0.00", None)] {
            assert_eq!(
                read(&format!("<e xmlns:x='{XMP}' x:Rating='{text}'/>")).unwrap(),
                rating
            );
        }
        for text in ["2.5", "5.00000000000000001", "NaN", "Infinity"] {
            assert!(read(&format!("<e xmlns:x='{XMP}' x:Rating='{text}'/>")).is_err());
        }
    }
    #[test]
    fn all_seven_states_roundtrip() {
        for r in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
            assert_eq!(read(&merge(None, r).unwrap()).unwrap(), r);
        }
    }
    #[test]
    fn foreign_prefixes_and_unknown_metadata_are_preserved() {
        let xml = format!(
            "<r:RDF xmlns:r='{RDF}'><r:Description xmlns:alt='{XMP}' xmlns:z='urn:other' alt:Rating='0' z:Note='家庭 &amp; 海边'/></r:RDF>"
        );
        let merged = merge(Some(&xml), Some(0)).unwrap();
        assert_eq!(merged, xml.replace("Rating='0'", "Rating='-1'"));
        assert_eq!(read(&merged).unwrap(), Some(0));
    }
    #[test]
    fn element_ratings_and_absence_are_supported() {
        let xml = format!(
            "<rdf:RDF xmlns:rdf='{RDF}' xmlns:xmp='{XMP}'><rdf:Description><xmp:Rating>5</xmp:Rating><Keep xmlns='urn:test'>text</Keep></rdf:Description></rdf:RDF>"
        );
        let out = merge(Some(&xml), None).unwrap();
        assert!(out.contains("<Keep xmlns='urn:test'>text</Keep>"));
        assert_eq!(read(&out).unwrap(), None);
        let empty = format!("<r:RDF xmlns:r='{RDF}'><r:Description/></r:RDF>");
        assert_eq!(
            read(&merge(Some(&empty), Some(4)).unwrap()).unwrap(),
            Some(4)
        );
    }
    #[test]
    fn invalid_or_conflicting_packets_are_not_silently_overwritten() {
        assert!(read("<broken>").is_err());
        let xml = format!("<e xmlns:x='{XMP}' x:Rating='4'><x:Rating>5</x:Rating></e>");
        assert!(merge(Some(&xml), Some(1)).is_err());
        assert!(read(&format!("<e xmlns:x='{XMP}' x:Rating='9'/>")).is_err());
    }
    #[test]
    fn rating_elements_keep_unknown_attributes_and_reject_complex_content() {
        let xml = format!(
            "<e xmlns:x='{XMP}' xmlns:other='urn:other'><x:Rating other:Note='keep'>5</x:Rating></e>"
        );
        let out = merge(Some(&xml), Some(0)).unwrap();
        assert_eq!(out, xml.replace(">5<", ">-1<"));
        let nested = format!("<e xmlns:x='{XMP}'><x:Rating><Other>5</Other></x:Rating></e>");
        assert!(merge(Some(&nested), Some(4)).is_err());
    }
}
