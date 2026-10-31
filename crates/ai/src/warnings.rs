//! Notices and warnings about using AI: before a cloud provider sees any text (privacy, costs,
//! answers can be wrong), and before and while a local model runs (memory, graphics memory, NPU
//! support, battery, disk space).
//!
//! Messages are English templates with `{name}` placeholders, filled from [`Warning::args`] after
//! the interface has translated the template.

use crate::hardware::{DeviceKind, DevicePreference, Hardware, LocalModel, choose_device, format_bytes};
use crate::provider::Provider;
use crate::settings::AiSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing; the user can dismiss it for good.
    Info,
    /// Something will be slower, costlier or less private than expected.
    Warning,
    /// AI can't be used like this until it is resolved.
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Stable id, used to dismiss [`Severity::Info`] notices.
    pub id: &'static str,
    pub severity: Severity,
    /// English text with `{name}` placeholders.
    pub template: &'static str,
    pub args: Vec<(&'static str, String)>,
}

impl Warning {
    fn new(id: &'static str, severity: Severity, template: &'static str) -> Self {
        Warning { id, severity, template, args: Vec::new() }
    }

    fn arg(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.args.push((name, value.into()));
        self
    }

    /// The text with its placeholders filled, using `template` (the translated template).
    pub fn fill(&self, template: &str) -> String {
        self.args.iter().fold(template.to_string(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
    }

    /// The English text.
    pub fn text(&self) -> String {
        self.fill(self.template)
    }
}

/// Every warning id, so the interface can check its translations and dismissals.
pub const IDS: [&str; 13] = [
    "no-key",
    "no-model",
    "azure-endpoint",
    "cloud-privacy",
    "cloud-costs",
    "can-be-wrong",
    "no-device",
    "device-fallback",
    "igpu-shares-ram",
    "cpu-slow",
    "ram-low",
    "battery",
    "disk-low",
];

/// Notices for using `settings.provider` from the cloud, before any text is sent. `has_key`:
/// whether a key is stored for it.
pub fn for_cloud(settings: &AiSettings, has_key: bool) -> Vec<Warning> {
    let p = settings.provider;
    let mut out = Vec::new();
    if !p.is_cloud() {
        return out;
    }
    if !has_key {
        out.push(
            Warning::new("no-key", Severity::Blocking, "No API key is stored for {provider}. Create one on the provider's page and paste it here.")
                .arg("provider", p.name()),
        );
    }
    if settings.model().is_none() {
        let template = if p == Provider::AzureOpenAi {
            "Enter the name of your Azure OpenAI deployment."
        } else {
            "Choose a model: load the list of models your account can use."
        };
        out.push(Warning::new("no-model", Severity::Blocking, template));
    }
    if p == Provider::AzureOpenAi && !settings.azure_endpoint.trim().starts_with("https://") {
        out.push(Warning::new("azure-endpoint", Severity::Blocking, "Enter your Azure OpenAI endpoint (https://<name>.openai.azure.com)."));
    }
    out.push(
        Warning::new(
            "cloud-privacy",
            Severity::Info,
            "Text you send to the AI is processed by {provider} ({region}). Send confidential documents only if you or your organisation allow it.",
        )
        .arg("provider", p.name())
        .arg("region", p.region()),
    );
    out.push(
        Warning::new("cloud-costs", Severity::Info, "{provider} bills each request to your API key. Long documents and long answers cost more.")
            .arg("provider", p.name()),
    );
    out.push(can_be_wrong());
    without_dismissed(out, settings)
}

fn can_be_wrong() -> Warning {
    Warning::new(
        "can-be-wrong",
        Severity::Info,
        "AI answers can be wrong or made up. Docufy offers them as suggestions you check before you accept them.",
    )
}

/// Warnings for running `model` locally on `hw` with `settings`.
pub fn for_local(settings: &AiSettings, hw: &Hardware, model: &LocalModel) -> Vec<Warning> {
    let mut out = Vec::new();
    let need = format_bytes(model.memory_bytes);
    if hw.disk_free.is_some_and(|free| free < model.download_bytes.saturating_add(model.download_bytes / 10)) {
        out.push(
            Warning::new("disk-low", Severity::Blocking, "{model} needs {size} of disk space; only {free} are free.")
                .arg("model", &model.name)
                .arg("size", format_bytes(model.download_bytes))
                .arg("free", format_bytes(hw.disk_free.unwrap_or(0))),
        );
    }
    let Some(device) = choose_device(hw, model, settings.device) else {
        let severity = if model.memory_bytes > hw.ram_total.saturating_mul(9) / 10 { Severity::Blocking } else { Severity::Warning };
        let template = if severity == Severity::Blocking {
            "{model} needs {need} of memory; this computer has {total}. Choose a smaller model."
        } else {
            "{model} needs {need} of free memory; {free} are free now. Close other programs or choose a smaller model."
        };
        out.push(
            Warning::new("ram-low", severity, template)
                .arg("model", &model.name)
                .arg("need", need)
                .arg("total", format_bytes(hw.ram_total))
                .arg("free", format_bytes(hw.ram_available)),
        );
        out.push(Warning::new("no-device", Severity::Blocking, "No device of this computer can run {model} right now.").arg("model", &model.name));
        return out;
    };
    if let DevicePreference::Only(wanted) = settings.device
        && wanted != device
    {
        out.push(fallback(hw, model, wanted, device));
    }
    match device {
        DeviceKind::IntegratedGpu => out.push(
            Warning::new(
                "igpu-shares-ram",
                Severity::Info,
                "Integrated graphics have no memory of their own: {model} uses {need} of the computer's RAM while it answers.",
            )
            .arg("model", &model.name)
            .arg("need", need),
        ),
        DeviceKind::Cpu => out.push(
            Warning::new(
                "cpu-slow",
                Severity::Warning,
                "{model} runs on the processor alone: answers come much more slowly than on a graphics card or NPU.",
            )
            .arg("model", &model.name),
        ),
        DeviceKind::DiscreteGpu | DeviceKind::Npu => {}
    }
    if hw.on_battery == Some(true) && matches!(device, DeviceKind::DiscreteGpu | DeviceKind::IntegratedGpu | DeviceKind::Cpu) {
        let template = if settings.pause_on_battery {
            "The computer runs on battery: local AI pauses until it is plugged in (Options › AI)."
        } else {
            "The computer runs on battery: a local model drains it quickly."
        };
        out.push(Warning::new("battery", Severity::Warning, template));
    }
    out.push(can_be_wrong());
    without_dismissed(out, settings)
}

/// Why the wanted device was not used.
fn fallback(hw: &Hardware, model: &LocalModel, wanted: DeviceKind, used: DeviceKind) -> Warning {
    let template = if !hw.has(wanted) {
        "This computer has no {wanted}; {model} runs on the {used} instead."
    } else if wanted == DeviceKind::Npu && !model.npu_ready {
        "{model} has no build for the NPU; it runs on the {used} instead."
    } else if wanted == DeviceKind::DiscreteGpu {
        "The graphics card has too little free memory for {model} ({need}); it runs on the {used} instead."
    } else {
        "There is too little free memory to run {model} on the {wanted} ({need}); it runs on the {used} instead."
    };
    Warning::new("device-fallback", Severity::Warning, template)
        .arg("model", &model.name)
        .arg("wanted", wanted.name())
        .arg("used", used.name())
        .arg("need", format_bytes(model.memory_bytes))
}

/// `out` without the notices the user dismissed. Warnings and blockers always stay.
fn without_dismissed(mut out: Vec<Warning>, settings: &AiSettings) -> Vec<Warning> {
    out.retain(|w| w.severity != Severity::Info || !settings.dismissed.contains(w.id));
    out.sort_by_key(|w| std::cmp::Reverse(w.severity));
    out
}

/// Whether nothing blocks using AI.
pub fn ready(warnings: &[Warning]) -> bool {
    warnings.iter().all(|w| w.severity != Severity::Blocking)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::tests::{GB, laptop, model};

    fn ids(w: &[Warning]) -> Vec<&'static str> {
        w.iter().map(|w| w.id).collect()
    }

    #[test]
    fn cloud_use_tells_about_privacy_costs_and_mistakes() {
        let s = AiSettings { enabled: true, ..AiSettings::default() };
        let w = for_cloud(&s, true);
        assert_eq!(ids(&w), ["cloud-privacy", "cloud-costs", "can-be-wrong"]);
        assert!(ready(&w));
        assert!(w[0].text().contains("Anthropic (Claude) (USA)"), "{}", w[0].text());
    }

    #[test]
    fn a_missing_key_or_model_blocks() {
        let s = AiSettings { provider: Provider::OpenAi, ..AiSettings::default() };
        let w = for_cloud(&s, false);
        assert_eq!(&ids(&w)[..2], ["no-key", "no-model"]);
        assert!(!ready(&w));
        let azure = AiSettings { provider: Provider::AzureOpenAi, ..AiSettings::default() };
        assert!(ids(&for_cloud(&azure, true)).contains(&"azure-endpoint"));
    }

    #[test]
    fn dismissed_notices_stay_hidden_but_blockers_never_do() {
        let mut s = AiSettings { provider: Provider::OpenAi, ..AiSettings::default() };
        s.dismissed.extend(["cloud-privacy".to_string(), "no-key".to_string()]);
        let w = ids(&for_cloud(&s, false));
        assert!(!w.contains(&"cloud-privacy"));
        assert!(w.contains(&"no-key"));
    }

    #[test]
    fn too_little_memory_is_a_warning_or_a_blocker() {
        let s = AiSettings::default();
        let hw = laptop();
        let w = for_local(&s, &hw, &model(25 * GB, false));
        assert_eq!(&ids(&w)[..2], ["ram-low", "no-device"]);
        assert_eq!(w[0].severity, Severity::Warning, "fits the computer, not what is free now");
        let w = for_local(&s, &hw, &model(31 * GB, false));
        assert_eq!(w[0].severity, Severity::Blocking);
        assert!(w[0].text().contains("31.0 GB"), "{}", w[0].text());
    }

    #[test]
    fn a_missed_device_choice_says_why() {
        let mut s = AiSettings { device: DevicePreference::Only(DeviceKind::Npu), ..AiSettings::default() };
        let hw = laptop();
        let w = for_local(&s, &hw, &model(4 * GB, false));
        assert!(w.iter().any(|w| w.id == "device-fallback" && w.text().contains("no build for the NPU")), "{w:?}");
        s.device = DevicePreference::Only(DeviceKind::DiscreteGpu);
        let w = for_local(&s, &hw, &model(10 * GB, false));
        let f = w.iter().find(|w| w.id == "device-fallback").unwrap();
        assert!(f.text().contains("too little free memory"), "{}", f.text());
        let mut no_npu = hw.clone();
        no_npu.npus.clear();
        s.device = DevicePreference::Only(DeviceKind::Npu);
        let w = for_local(&s, &no_npu, &model(4 * GB, true));
        assert!(w.iter().any(|w| w.text().contains("has no NPU")), "{w:?}");
    }

    #[test]
    fn integrated_graphics_cpu_and_battery_are_explained() {
        let s = AiSettings { device: DevicePreference::Only(DeviceKind::IntegratedGpu), ..AiSettings::default() };
        let mut hw = laptop();
        hw.on_battery = Some(true);
        let w = ids(&for_local(&s, &hw, &model(4 * GB, false)));
        assert!(w.contains(&"igpu-shares-ram") && w.contains(&"battery"), "{w:?}");
        let s = AiSettings { device: DevicePreference::Only(DeviceKind::Cpu), ..AiSettings::default() };
        assert!(ids(&for_local(&s, &hw, &model(4 * GB, false))).contains(&"cpu-slow"));
    }

    #[test]
    fn a_full_disk_blocks_the_download() {
        let mut hw = laptop();
        hw.disk_free = Some(GB);
        let w = for_local(&AiSettings::default(), &hw, &model(4 * GB, false));
        assert_eq!(w[0].id, "disk-low");
        assert!(!ready(&w));
    }

    #[test]
    fn every_id_is_listed() {
        let mut seen: Vec<&str> = Vec::new();
        for p in Provider::ALL {
            let s = AiSettings { provider: p, ..AiSettings::default() };
            seen.extend(ids(&for_cloud(&s, false)));
        }
        let mut hw = laptop();
        hw.on_battery = Some(true);
        hw.disk_free = Some(GB);
        for pref in [DevicePreference::Auto, DevicePreference::Only(DeviceKind::Npu), DevicePreference::Only(DeviceKind::Cpu)] {
            let s = AiSettings { device: pref, ..AiSettings::default() };
            for m in [model(4 * GB, false), model(10 * GB, false), model(25 * GB, false)] {
                seen.extend(ids(&for_local(&s, &hw, &m)));
            }
        }
        for w in seen {
            assert!(IDS.contains(&w), "{w}");
        }
    }
}
