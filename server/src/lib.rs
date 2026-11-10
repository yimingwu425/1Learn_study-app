use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaperName {
    pub subject: String,
    pub exam_code: String,
    pub role: String,
}
pub fn identify_paper(filename: &str) -> Option<PaperName> {
    let re = Regex::new(r"(?i)^(9702|9708)_([msw])(\d{2})_(qp|ms)_(\d{2})\.pdf$").unwrap();
    let c = re.captures(filename)?;
    Some(PaperName {
        subject: c[1].to_string(),
        exam_code: format!("{}_{}{}_{}", &c[1], &c[2].to_lowercase(), &c[3], &c[5]),
        role: c[4].to_lowercase(),
    })
}
pub fn validate_candidate(c: &Value, qp_pages: u64, ms_pages: Option<u64>) -> Result<(), String> {
    let no = c.get("question_no").and_then(Value::as_str).unwrap_or("");
    if no.trim().is_empty()
        || no.len() > 100
        || no.chars().any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return Err("请填写有效题号".into());
    }
    let validate_pages = |field: &str, max: u64, required: bool| -> Result<(), String> {
        let Some(value) = c.get(field) else {
            return if required {
                Err("缺少题面页码".into())
            } else {
                Ok(())
            };
        };
        let pages = value.as_array().ok_or("页码应为数组")?;
        if required && pages.is_empty() {
            return Err("至少选择一页题面".into());
        }
        let mut seen = std::collections::HashSet::new();
        for p in pages {
            let n = p.as_u64().ok_or("页码必须是正整数")?;
            if n == 0 || n > max {
                return Err("页码超出PDF范围，或未关联评分标准".into());
            }
            if !seen.insert(n) {
                return Err("页码不能重复".into());
            }
        }
        Ok(())
    };
    validate_pages("qp_pages", qp_pages, true)?;
    validate_pages("ms_pages", ms_pages.unwrap_or(0), false)?;
    let text = c.get("text").and_then(Value::as_str).unwrap_or("");
    if text.trim().is_empty() {
        return Err("题干不能为空，请检查提取结果".into());
    }
    if text.chars().count() > 50000 {
        return Err("单题文字不能超过50000字".into());
    }
    if let Some(m) = c.get("marks").filter(|m| !m.is_null()) {
        if m.as_f64()
            .is_none_or(|x| !x.is_finite() || x < 0.0 || x > 1000.0)
        {
            return Err("分值应为0至1000之间的数值".into());
        }
    }
    if let Some(topics) = c.get("topics") {
        let ts = topics.as_array().ok_or("专题标签应为字符串数组")?;
        if ts.len() > 30
            || ts.iter().any(|t| {
                t.as_str()
                    .is_none_or(|s| s.is_empty() || s.chars().count() > 100)
            })
        {
            return Err("最多30个专题标签，每个不超过100字".into());
        }
    }
    Ok(())
}
pub fn valid_attempt_status(s: &str) -> bool {
    matches!(s, "independent" | "hinted" | "retry")
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn paper_naming() {
        let p = identify_paper("9702_m25_qp_22.pdf").unwrap();
        assert_eq!(p.exam_code, "9702_m25_22");
        assert_eq!(p.role, "qp");
        assert!(identify_paper("../../9702_m25_qp_22.pdf").is_none());
        assert!(identify_paper("random.pdf").is_none());
    }
    #[test]
    fn reject_bad_pages() {
        assert!(validate_candidate(
            &json!({"question_no":"1","qp_pages":[0],"text":"test"}),
            2,
            None
        )
        .is_err());
        assert!(validate_candidate(
            &json!({"question_no":"1","qp_pages":[1],"ms_pages":[1],"text":"test"}),
            2,
            None
        )
        .is_err());
    }
    #[test]
    fn accept_reviewed_mapping() {
        assert!(validate_candidate(
            &json!({"question_no":"1","qp_pages":[1,2],"ms_pages":[3],"text":"test"}),
            2,
            Some(3)
        )
        .is_ok());
    }
    #[test]
    fn reject_ambiguous_mapping() {
        for extra in [
            json!({"qp_pages":[1,1]}),
            json!({"ms_pages":"2"}),
            json!({"topics":[{}]}),
            json!({"marks":-1}),
            json!({"question_no":"../1"}),
        ] {
            let mut q = json!({"question_no":"1","qp_pages":[1],"text":"test"});
            for (k, v) in extra.as_object().unwrap() {
                q[k] = v.clone()
            }
            assert!(validate_candidate(&q, 2, Some(2)).is_err());
        }
    }
    #[test]
    fn self_assessment_only() {
        assert!(valid_attempt_status("retry"));
        assert!(!valid_attempt_status("ai_passed"));
    }
}
