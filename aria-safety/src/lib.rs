use aho_corasick::AhoCorasick;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanResult {
    Clean,
    Alert(Vec<String>),
}

#[derive(Clone)]
pub struct DfaFirewall {
    scanner: AhoCorasick,
    patterns: Vec<String>,
}

impl DfaFirewall {
    /// Create a new DFA firewall with the given patterns.
    pub fn new(patterns: Vec<String>) -> Self {
        // We compile a case-insensitive DFA string matcher.
        let scanner = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&patterns)
            .expect("Failed to build AhoCorasick firewall");
        Self { scanner, patterns }
    }

    /// Scan incoming user prompts for prompt injection signatures or bad keywords.
    pub fn scan_ingress(&self, prompt: &str) -> ScanResult {
        self.scan_text(prompt)
    }

    /// Scan outgoing responses or network payloads for credential exfiltration.
    pub fn scan_egress(&self, response: &str) -> ScanResult {
        self.scan_text(response)
    }

    fn scan_text(&self, text: &str) -> ScanResult {
        let mut alerts = Vec::new();
        for mat in self.scanner.find_iter(text) {
            let pattern_str = &self.patterns[mat.pattern()];
            if !alerts.contains(pattern_str) {
                alerts.push(pattern_str.clone());
            }
        }
        if alerts.is_empty() {
            ScanResult::Clean
        } else {
            ScanResult::Alert(alerts)
        }
    }
}
