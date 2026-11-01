//! The models Docufy offers on this computer, and the runtime that runs them: where each file comes
//! from (a Hugging Face repository pinned to a commit, Microsoft's release and NuGet packages), its
//! size and its SHA-256, so a download is checked before it is used. Entries come from
//! `crates/ai/tools/gen-catalog.sh <repo> <folder>`, which reads a repository's file list.

/// One file to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// Which build of a model: for the graphics chip (DirectML) or the processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Build {
    Gpu,
    Cpu,
}

impl Build {
    pub fn id(self) -> &'static str {
        match self {
            Build::Gpu => "gpu",
            Build::Cpu => "cpu",
        }
    }

    pub fn from_id(id: &str) -> Option<Build> {
        [Build::Gpu, Build::Cpu].into_iter().find(|b| b.id() == id)
    }

    /// The ONNX Runtime execution provider the build runs with.
    pub fn provider(self) -> &'static str {
        match self {
            Build::Gpu => "dml",
            Build::Cpu => "cpu",
        }
    }
}

/// A model's files for one build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    pub build: Build,
    pub repo: &'static str,
    pub revision: &'static str,
    /// The folder inside the repository ("" for its root).
    pub dir: &'static str,
    pub files: &'static [ModelFile],
}

impl Variant {
    /// The bytes to download.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Where `file` is downloaded from.
    pub fn url(&self, file: &ModelFile) -> String {
        let dir = if self.dir.is_empty() { String::new() } else { format!("{}/", self.dir) };
        format!("https://huggingface.co/{}/resolve/{}/{dir}{}", self.repo, self.revision, file.name)
    }
}

/// How a model expects a conversation to be written ([`crate::local::template`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Phi3,
    Qwen2,
    Mistral,
    Llama3,
}

/// A model on offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogModel {
    /// The id kept in the settings.
    pub id: &'static str,
    pub name: &'static str,
    pub maker: &'static str,
    pub parameters: &'static str,
    pub licence: &'static str,
    pub licence_url: &'static str,
    /// What it is good for (English; the interface translates it).
    pub about: &'static str,
    pub family: Family,
    /// The longest conversation it takes, in tokens.
    pub context: u32,
    pub variants: &'static [Variant],
}

impl CatalogModel {
    pub fn variant(&self, build: Build) -> Option<&'static Variant> {
        self.variants.iter().find(|v| v.build == build)
    }

    /// What the model needs while it answers: its weights plus a working margin for the
    /// conversation, for the largest build (the memory warnings stay on the safe side).
    pub fn memory_bytes(&self) -> u64 {
        let weights = self.variants.iter().map(Variant::bytes).max().unwrap_or(0);
        weights + weights / 5 + 300_000_000
    }

    /// For choosing a device and the memory warnings ([`crate::hardware`], [`crate::warnings`]).
    pub fn local_model(&self) -> crate::hardware::LocalModel {
        crate::hardware::LocalModel {
            id: self.id.to_string(),
            name: self.name.to_string(),
            download_bytes: self.variants.iter().map(Variant::bytes).max().unwrap_or(0),
            memory_bytes: self.memory_bytes(),
            npu_ready: false,
        }
    }
}

/// The model by its settings id.
pub fn model(id: &str) -> Option<&'static CatalogModel> {
    MODELS.iter().find(|m| m.id == id)
}

/// The model offered first: small enough for most computers, good German.
pub const DEFAULT_MODEL: &str = "phi-4-mini";

pub static MODELS: [CatalogModel; 5] = [
    CatalogModel {
        id: "phi-4-mini",
        name: "Phi-4-mini",
        maker: "Microsoft",
        parameters: "3.8 B",
        licence: "MIT",
        licence_url: "https://huggingface.co/microsoft/Phi-4-mini-instruct-onnx/blob/main/LICENSE",
        about: "Good all-rounder with solid German; needs about 4 GB of graphics memory.",
        family: Family::Phi3,
        context: 131_072,
        variants: &[
            Variant {
                build: Build::Gpu,
                // https://huggingface.co/microsoft/Phi-4-mini-instruct-onnx/tree/fc04c8f93df696602fd9f300a30d1bf2e3081347/gpu/gpu-int4-rtn-block-32
                repo: "microsoft/Phi-4-mini-instruct-onnx",
                revision: "fc04c8f93df696602fd9f300a30d1bf2e3081347",
                dir: "gpu/gpu-int4-rtn-block-32",
                files: &[
                    ModelFile { name: "added_tokens.json", size: 249, sha256: "d4f2aceb0f20b71dd1f4bcc7e052e4412946bf281840b8f83d39f259571af486" },
                    ModelFile { name: "config.json", size: 2504, sha256: "ac65d86061d3d0d704ee2511fd0eb8713ef19eb6eedba17c3080a4165d5b933b" },
                    ModelFile { name: "genai_config.json", size: 1568, sha256: "3cdbd2ff0b2cda7829b262cda331a72b99daaa758d55bb056bb4d098d22b1140" },
                    ModelFile { name: "merges.txt", size: 2418348, sha256: "856ce61180bb689282eed6b3a6838bb1f438399be23aefe9d20eb379791fb4ad" },
                    ModelFile { name: "model.onnx", size: 286690, sha256: "f8ff300b85719f79f1c220c88575626c62cc772a228244a6b96031d1a8d25250" },
                    ModelFile {
                        name: "model.onnx.data",
                        size: 3413194752,
                        sha256: "b32f38f9211040b01c1483759cef7a6cce28df9f83b6b8bb91a474575d559d0f",
                    },
                    ModelFile {
                        name: "special_tokens_map.json",
                        size: 587,
                        sha256: "aff38493227d813e29fcf8406e8e90062f1f031aa47d589325e9c31d89ac7cc3",
                    },
                    ModelFile { name: "tokenizer.json", size: 15524095, sha256: "382cc235b56c725945e149cc25f191da667c836655efd0857b004320e90e91ea" },
                    ModelFile {
                        name: "tokenizer_config.json",
                        size: 2960,
                        sha256: "c565326a315fbe62cda093a59d298828c8f3f823122661325f41f3ba577a7dec",
                    },
                    ModelFile { name: "vocab.json", size: 3910310, sha256: "6cb65a857824fa6615bb1782d95d882617a8bbce1da0317118586b36f39e98bd" },
                ],
            },
            Variant {
                build: Build::Cpu,
                // https://huggingface.co/microsoft/Phi-4-mini-instruct-onnx/tree/fc04c8f93df696602fd9f300a30d1bf2e3081347/cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4
                repo: "microsoft/Phi-4-mini-instruct-onnx",
                revision: "fc04c8f93df696602fd9f300a30d1bf2e3081347",
                dir: "cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4",
                files: &[
                    ModelFile { name: "added_tokens.json", size: 249, sha256: "d4f2aceb0f20b71dd1f4bcc7e052e4412946bf281840b8f83d39f259571af486" },
                    ModelFile { name: "config.json", size: 2504, sha256: "ac65d86061d3d0d704ee2511fd0eb8713ef19eb6eedba17c3080a4165d5b933b" },
                    ModelFile { name: "genai_config.json", size: 1520, sha256: "0fcfa1e663f2bc867f8dc62fae65dd0924f0a4d68b43d1234df742dd19171470" },
                    ModelFile { name: "merges.txt", size: 2418348, sha256: "856ce61180bb689282eed6b3a6838bb1f438399be23aefe9d20eb379791fb4ad" },
                    ModelFile { name: "model.onnx", size: 52118230, sha256: "701aa5d185b6a782bc27104a990dd5b634fa507840b7c42f7ee6f1fb812d0b83" },
                    ModelFile {
                        name: "model.onnx.data",
                        size: 4856573952,
                        sha256: "cb0267fa60befa1a4ade8c98b6d32a3d67f51abbd307c7f793f132e8d9092131",
                    },
                    ModelFile {
                        name: "special_tokens_map.json",
                        size: 587,
                        sha256: "aff38493227d813e29fcf8406e8e90062f1f031aa47d589325e9c31d89ac7cc3",
                    },
                    ModelFile { name: "tokenizer.json", size: 15524095, sha256: "382cc235b56c725945e149cc25f191da667c836655efd0857b004320e90e91ea" },
                    ModelFile {
                        name: "tokenizer_config.json",
                        size: 2960,
                        sha256: "c565326a315fbe62cda093a59d298828c8f3f823122661325f41f3ba577a7dec",
                    },
                    ModelFile { name: "vocab.json", size: 3910310, sha256: "6cb65a857824fa6615bb1782d95d882617a8bbce1da0317118586b36f39e98bd" },
                ],
            },
        ],
    },
    CatalogModel {
        id: "qwen2.5-1.5b",
        name: "Qwen2.5 1.5B",
        maker: "Alibaba",
        parameters: "1.5 B",
        licence: "Apache 2.0",
        licence_url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/blob/main/LICENSE",
        about: "Small and quick on the processor, for computers without a graphics card; simpler answers.",
        family: Family::Qwen2,
        context: 32_768,
        variants: &[Variant {
            build: Build::Cpu,
            // https://huggingface.co/keisuke-miyako/Qwen2.5-1.5B-Instruct-onnx-int4/tree/88736c2e1167ab73c7ed9e82d0fe546273c5e03d/
            repo: "keisuke-miyako/Qwen2.5-1.5B-Instruct-onnx-int4",
            revision: "88736c2e1167ab73c7ed9e82d0fe546273c5e03d",
            dir: "",
            files: &[
                ModelFile { name: "added_tokens.json", size: 605, sha256: "58b54bbe36fc752f79a24a271ef66a0a0830054b4dfad94bde757d851968060b" },
                ModelFile { name: "config.json", size: 660, sha256: "98d2ff8cc47488d08a2b0b3acf4eb99ef210779b42bd48605f6b8e36acdbf670" },
                ModelFile { name: "genai_config.json", size: 1519, sha256: "d8e0b9cd680e9a665a06087342d96649b7012c4a9b18a788d945c74b793c47ae" },
                ModelFile { name: "merges.txt", size: 1671853, sha256: "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5" },
                ModelFile { name: "model.onnx", size: 220081, sha256: "d11cc30e2f7bd5219da788572638e5b351487ebf28971da7a3371688479b576a" },
                ModelFile { name: "model.onnx.data", size: 1919811584, sha256: "e79ff7b9be7e4f9583d1c09ea64676bf56f13cdb028b87f9df68994cb3ec5b2b" },
                ModelFile { name: "special_tokens_map.json", size: 613, sha256: "76862e765266b85aa9459767e33cbaf13970f327a0e88d1c65846c2ddd3a1ecd" },
                ModelFile { name: "tokenizer.json", size: 11421896, sha256: "9c5ae00e602b8860cbd784ba82a8aa14e8feecec692e7076590d014d7b7fdafa" },
                ModelFile { name: "tokenizer_config.json", size: 4686, sha256: "0a04a9d7d4a62b28482bdfe726c122756de85714fb64166ace92ae75b8f57614" },
                ModelFile { name: "vocab.json", size: 2776833, sha256: "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910" },
            ],
        }],
    },
    CatalogModel {
        id: "qwen2.5-3b",
        name: "Qwen2.5 3B",
        maker: "Alibaba",
        parameters: "3 B",
        licence: "Qwen Research License",
        licence_url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct/blob/main/LICENSE",
        about: "Better answers than the 1.5B, on the processor; for personal and research use (see the licence).",
        family: Family::Qwen2,
        context: 32_768,
        variants: &[Variant {
            build: Build::Cpu,
            // https://huggingface.co/keisuke-miyako/Qwen2.5-3B-Instruct-onnx-int4/tree/2da3b0c0a6fa5af60f169ec38fcc68adc1a45b92/
            repo: "keisuke-miyako/Qwen2.5-3B-Instruct-onnx-int4",
            revision: "2da3b0c0a6fa5af60f169ec38fcc68adc1a45b92",
            dir: "",
            files: &[
                ModelFile { name: "added_tokens.json", size: 605, sha256: "58b54bbe36fc752f79a24a271ef66a0a0830054b4dfad94bde757d851968060b" },
                ModelFile { name: "config.json", size: 661, sha256: "eed00b17e22553979d090fa492e587e92885e328914c8e0b0b78f0a0d3576b3b" },
                ModelFile { name: "genai_config.json", size: 1519, sha256: "54b5f2297aada763cf8ce7c55028f81f82b82f85ef9c2afc7aa7204a681b018a" },
                ModelFile { name: "merges.txt", size: 1671853, sha256: "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5" },
                ModelFile { name: "model.onnx", size: 282531, sha256: "1fbac4b7498d15207a1bd9857c0ed9e7f9428a5a91d7943899b1b0e4690bec2e" },
                ModelFile { name: "model.onnx.data", size: 3191013376, sha256: "a60f679e1e187c4734cdb95114f8aa30e9afaec7193c23a0ef6a67923e69c503" },
                ModelFile { name: "special_tokens_map.json", size: 613, sha256: "76862e765266b85aa9459767e33cbaf13970f327a0e88d1c65846c2ddd3a1ecd" },
                ModelFile { name: "tokenizer.json", size: 11421896, sha256: "9c5ae00e602b8860cbd784ba82a8aa14e8feecec692e7076590d014d7b7fdafa" },
                ModelFile { name: "tokenizer_config.json", size: 4686, sha256: "0a04a9d7d4a62b28482bdfe726c122756de85714fb64166ace92ae75b8f57614" },
                ModelFile { name: "vocab.json", size: 2776833, sha256: "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910" },
            ],
        }],
    },
    CatalogModel {
        id: "mistral-7b",
        name: "Mistral 7B Instruct",
        maker: "Mistral AI (EU)",
        parameters: "7 B",
        licence: "Apache 2.0",
        licence_url: "https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3",
        about: "The strongest of these models; needs about 6 GB of graphics memory or a lot of RAM.",
        family: Family::Mistral,
        context: 32_768,
        variants: &[
            Variant {
                build: Build::Gpu,
                // https://huggingface.co/EmbeddedLLM/mistral-7b-instruct-v0.3-int4-onnx-directml/tree/97e9e77ca3023f1954f752143dd3bd5aac669398/
                repo: "EmbeddedLLM/mistral-7b-instruct-v0.3-int4-onnx-directml",
                revision: "97e9e77ca3023f1954f752143dd3bd5aac669398",
                dir: "",
                files: &[
                    ModelFile { name: "config.json", size: 601, sha256: "affafc6478ec0fd07a32f0ca57aa2fc57743f4d17d6730f86a96ac24d1507f99" },
                    ModelFile { name: "genai_config.json", size: 1604, sha256: "1ce3a4af57f78f085d8035dde328e8e90f44df6747f584fbfd48c7f9428f801e" },
                    ModelFile { name: "model.onnx", size: 278364, sha256: "3796575b4c1da99e1c8bf4044807df6fef2742153d586c75ed849bd2cb5111ef" },
                    ModelFile {
                        name: "model.onnx.data",
                        size: 4278722560,
                        sha256: "50fe89b4d9e901d103a8def6fb35821eb45faef88dae713d6286b0bda513a98b",
                    },
                    ModelFile {
                        name: "special_tokens_map.json",
                        size: 414,
                        sha256: "6fa06efa2785e450051989a6f8fb4416b10149ded485ddd3f127a40734f5cfd0",
                    },
                    ModelFile { name: "tokenizer.json", size: 1961548, sha256: "e553af6fff7d7ad76e830608b218c5c0b0822998d5a1a96099a74cd3c1cb1a49" },
                    ModelFile { name: "tokenizer.model", size: 587404, sha256: "37f00374dea48658ee8f5d0f21895b9bc55cb0103939607c8185bfd1c6ca1f89" },
                    ModelFile {
                        name: "tokenizer_config.json",
                        size: 137190,
                        sha256: "489746d6758f2e62d01eb30b7b6486e60ebae4728a97cbce4905a2eb7264f5fd",
                    },
                ],
            },
            Variant {
                build: Build::Cpu,
                // https://huggingface.co/EmbeddedLLM/mistral-7b-instruct-v0.3-onnx-cpu-int4-rtn-block-32/tree/7e65d8e4377f2ea77b89d6e862309b0d0343a20e/
                repo: "EmbeddedLLM/mistral-7b-instruct-v0.3-onnx-cpu-int4-rtn-block-32",
                revision: "7e65d8e4377f2ea77b89d6e862309b0d0343a20e",
                dir: "",
                files: &[
                    ModelFile { name: "config.json", size: 626, sha256: "bcfd55d5d46129b0e0bea9109e5873a4f0a6ea19d3cecc89afb083b6a0de6a2a" },
                    ModelFile { name: "genai_config.json", size: 1509, sha256: "3d7bea855f1bf301850186a9f31cf79c79c543f1e7967c81531630803e35438a" },
                    ModelFile { name: "model.onnx", size: 212044, sha256: "6f759d74c6c4fdaedede14717d4d5a00922ae7fb1993d86e553b818c4c6bb146" },
                    ModelFile {
                        name: "model.onnx.data",
                        size: 5000675328,
                        sha256: "7f7c99565ad875c283ee02e47cc8a80b51471b34a257db5d073a53f580da3e94",
                    },
                    ModelFile {
                        name: "special_tokens_map.json",
                        size: 437,
                        sha256: "b779ce0b2be00f7cfd9d6188ed88e94700dafc1a7138fef8fca88f06aca11dac",
                    },
                    ModelFile { name: "tokenizer.json", size: 2060340, sha256: "1435231f32be9c6d98c70ab6c8b9ef7aa6b7725acb5f96baa3eeddc39330e6c5" },
                    ModelFile { name: "tokenizer.model", size: 587404, sha256: "37f00374dea48658ee8f5d0f21895b9bc55cb0103939607c8185bfd1c6ca1f89" },
                    ModelFile {
                        name: "tokenizer_config.json",
                        size: 143377,
                        sha256: "72c22c6ceddc1c7ea7b89db2924dfe52dfc382ff32b844a08973aa980f58a895",
                    },
                ],
            },
        ],
    },
    CatalogModel {
        id: "llama-3.2-3b",
        name: "Llama 3.2 3B Instruct",
        maker: "Meta",
        parameters: "3 B",
        licence: "Llama 3.2 Community License",
        licence_url: "https://huggingface.co/meta-llama/Llama-3.2-3B-Instruct/blob/main/LICENSE.txt",
        about: "Compact and multilingual, on the processor; Meta's licence has conditions of its own.",
        family: Family::Llama3,
        context: 131_072,
        variants: &[Variant {
            build: Build::Cpu,
            // https://huggingface.co/onnx-community/Llama-3.2-3B-Instruct-GENAI-ONNX/tree/5db5cdb5b0c8c440264ca0f16f5ec3351e753add/cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4
            repo: "onnx-community/Llama-3.2-3B-Instruct-GENAI-ONNX",
            revision: "5db5cdb5b0c8c440264ca0f16f5ec3351e753add",
            dir: "cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4",
            files: &[
                ModelFile { name: "config.json", size: 877, sha256: "c2bb918e2e232a1b2630ac1a0d51d771bda001ce83c4e330badfd7e2dff564cc" },
                ModelFile { name: "genai_config.json", size: 1540, sha256: "c86500138845dda1b2a9e2a626a5f72e57ff9bd90012d6bff5195f5fce7b4ff8" },
                ModelFile { name: "model.onnx", size: 185824, sha256: "b159a71e593d75290c6827b2d67430b469947bbf9b807fd4e291ac80089caa65" },
                ModelFile { name: "model.onnx.data", size: 3651678208, sha256: "0a6801080cf0bd335d1f35909b574e70f6cdc0faadb3735289dee96fcbd19ec6" },
                ModelFile { name: "special_tokens_map.json", size: 296, sha256: "6f38c73729248f6c127296386e3cdde96e254636cc58b4169d3fd32328d9a8ec" },
                ModelFile { name: "tokenizer.json", size: 9085657, sha256: "79e3e522635f3171300913bb421464a87de6222182a0570b9b2ccba2a964b2b4" },
                ModelFile { name: "tokenizer_config.json", size: 54528, sha256: "9823dcfdc1121869029da45192238e85cf44f0b232a6d9dc20e4fe6f4242a14e" },
            ],
        }],
    },
];
