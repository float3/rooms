//! The statements a room can be dealt, by topic.
//!
//! Each is written to be disagreed with: a claim a thoughtful person could
//! hold at either end of the scale. A statement starting with "% " is a
//! question answered with a probability instead of agreement, and is
//! always "What is the chance that ...?", so that it can be turned into a
//! claim when a room answers everything as agreement. The first two topics' wording started
//! from the speed updating sheets EA groups have used; the rest are ours.
//! Rooms copy the statements they are dealt, so editing this list does not
//! change a room that is already open.

pub struct Topic {
    pub key: &'static str,
    pub name: &'static str,
    /// Whether a new room starts with this topic ticked.
    pub default: bool,
    pub statements: &'static [&'static str],
}

pub const TOPICS: &[Topic] = &[
    Topic {
        key: "ea",
        name: "Effective altruism",
        default: true,
        statements: &[
            "Effective charities are wildly more impactful than the vast majority of charities.",
            "% What is the chance that EA is right about what to prioritise to do the most good?",
            "Earning to give is something more EAs should consider.",
            "Changing policy is the most effective way to make a positive impact.",
            "Community building is the most effective way to make a positive impact.",
            "Research is the most effective way to make a positive impact.",
            "Earning to give is the most effective way to make a positive impact.",
            "Global health and wellbeing is the most important cause area.",
            "EA spends too little time thinking about systemic change.",
            "EA has too many people working on radical ideas.",
            "Those in the top 1% of the global income distribution should give away more than 10% of it.",
            "When choosing a charity, we should go by the data rather than our emotions and intuitions.",
            "My local group has changed my career plans.",
            "EA is better off without the label \"EA\".",
            "Most people who call themselves EAs would do more good if they took a normal job and donated.",
            "Value drift is real: most people become less altruistic as they age.",
            "Billionaires should not be allowed to exist.",
            "Universal basic income should be introduced.",
            "In five years I will have a significantly different view of what the most pressing problem is.",
            "I need to spend much more time thinking about the world's most pressing problems.",
            "Longtermism is the most important idea EA has produced.",
        ],
    },
    Topic {
        key: "ai",
        name: "AI safety",
        default: true,
        statements: &[
            "AI safety is the most important cause area.",
            "Misaligned AI is a significant existential threat.",
            "% What is the chance that AI causes human extinction this century?",
            "AI will ultimately create more problems than it solves.",
            "% What is the chance that we will have AGI before 2035?",
            "Frontier AI development should be paused until we know how to make it safe.",
            "% What is the chance that alignment is solved before it is needed?",
            "% What is the chance that a pause on frontier AI is agreed internationally by 2030?",
            "Working at a frontier AI lab on safety does more good than harm.",
            "Open-weight models make the world safer on balance.",
            "Current large language models understand what they are saying.",
            "Alignment is mainly a technical problem rather than a political one.",
            "Governments are more likely to make AI go well than AI labs are.",
            "A superintelligence would by default pursue goals we did not intend.",
            "AI will cause mass unemployment within twenty years.",
            "Interpretability research will be the key to making AI safe.",
            "It would be fine for humanity to be succeeded by AIs that are conscious and flourishing.",
            "International coordination on AI is achievable before it is too late.",
        ],
    },
    Topic {
        key: "vegan",
        name: "Animals and veganism",
        default: true,
        statements: &[
            "I am vegan, or I should be.",
            "Factory farming is the biggest moral atrocity happening in the world right now.",
            "Meat eating will be seen as barbaric in the future.",
            "Animal welfare is the most important cause area.",
            "Eating meat from animals that had good lives is morally fine.",
            "Fish feel pain in a way that matters morally.",
            "Insect suffering should be a serious moral priority.",
            "Cultivated meat will replace most animal farming within thirty years.",
            "% What is the chance that factory farming is banned in the EU by 2050?",
            "Reducetarianism does more good than promoting strict veganism.",
            "Wild animal suffering is a problem we should try to solve.",
            "A chicken's suffering matters at least a tenth as much as a human's.",
            "Keeping pets is ethically questionable.",
            "Vegetarianism without veganism is morally inconsistent.",
        ],
    },
    Topic {
        key: "metaethics",
        name: "Metaethics",
        default: true,
        statements: &[
            "There are objective moral facts.",
            "% What is the chance that moral realism is true?",
            "Morality is just a preference.",
            "Moral claims can be true or false in the same way factual claims can.",
            "If God does not exist, nothing is morally forbidden.",
            "A sufficiently intelligent being would converge on the right morality.",
            "Moral intuitions are evidence about what is right.",
            "Moral progress is real, not just change.",
            "Two people who disagree about a moral question can both be right.",
            "We should approach ethics from first principles rather than case-by-case intuitions.",
            "Evolution explains our moral beliefs well enough that we should doubt them.",
            "Moral uncertainty should change how we act, not just what we believe.",
            "\"Good\" means nothing more than \"approved of by some group\".",
        ],
    },
    Topic {
        key: "ethics",
        name: "Ethics",
        default: true,
        statements: &[
            "People are morally obligated to do good, not just to avoid harm.",
            "Future lives should carry significant weight in our moral decisions.",
            "Lying is always wrong.",
            "Utilitarianism is roughly correct.",
            "% What is the chance that utilitarianism is the correct moral theory?",
            "Bringing a happy person into existence is a good thing.",
            "Human reproduction is morally good.",
            "It is wrong to spend money on luxuries while others die of preventable diseases.",
            "Killing is worse than letting die.",
            "Pushing the fat man off the bridge to save five is the right thing to do.",
            "Some acts are wrong whatever their consequences.",
            "We have stronger obligations to our family and fellow citizens than to strangers.",
            "A world with ten billion barely happy people is better than one with one billion very happy people.",
            "Intentions matter morally even when they change nothing.",
            "Humans 50 years from now will look back and say our society normalised moral atrocities.",
            "Emotional intelligence matters more than IQ.",
        ],
    },
    Topic {
        key: "mind",
        name: "Consciousness and mind",
        default: true,
        statements: &[
            "Consciousness can be fully explained by physical processes.",
            "There is something it is like to be a bat.",
            "A perfect simulation of a brain would be conscious.",
            "Current AI systems have some form of consciousness.",
            "% What is the chance that today's large language models are conscious?",
            "% What is the chance that an AI is conscious by 2040?",
            "% What is the chance that a honeybee is conscious?",
            "% What is the chance that a fish feels pain?",
            "We will never know for sure whether an AI is conscious.",
            "Free will is an illusion.",
            "Free will is compatible with determinism.",
            "The hard problem of consciousness is a real problem, not a confusion.",
            "Consciousness is an illusion.",
            "Panpsychism, the view that consciousness is everywhere in matter, is plausible.",
            "Plants are conscious in some meaningful sense.",
            "If you were teleported by being scanned and rebuilt, you would survive.",
            "Personal identity is not what matters for survival.",
            "Uploading your mind to a computer would keep you alive.",
            "Animals are conscious in the same basic way humans are.",
            "Qualia exist.",
            "A philosophical zombie is conceivable.",
        ],
    },
    Topic {
        key: "metaphysics",
        name: "Metaphysics",
        default: false,
        statements: &[
            "We are probably living in a simulation.",
            "% What is the chance that we are living in a simulation?",
            "% What is the chance that God exists?",
            "The past, present and future are all equally real.",
            "Mathematical objects exist independently of minds.",
            "Every event has a cause.",
            "The many-worlds interpretation of quantum mechanics is correct.",
            "There is a fact of the matter about whether something is a heap.",
            "God exists.",
            "Laws of nature describe what happens rather than make it happen.",
            "Something can come from nothing.",
            "The universe has a purpose.",
            "Causation is an objective feature of the world, not just something we project onto it.",
            "Abstract things like numbers and properties are real.",
            "Time really passes; it is not just how things seem to us.",
        ],
    },
    Topic {
        key: "epistemics",
        name: "Knowledge and rationality",
        default: false,
        statements: &[
            "Bayesian reasoning is the correct way to handle uncertainty.",
            "Most people would be better off trusting expert consensus over their own reasoning.",
            "You should be able to put a probability on any claim.",
            "If you cannot change my mind, I should not change yours.",
            "Intuition is a reliable guide in areas where you have no data.",
            "Rationalists overestimate how much rationality improves decisions.",
            "It is possible to know things with certainty.",
            "Arguing with people you disagree with usually changes nobody's mind.",
            "Prediction markets should be legal and widely used.",
            "% What is the chance that a typical published psychology finding would replicate?",
            "Calibration is more important than being right.",
            "Beliefs should be held for their accuracy alone, not for how they make you feel.",
        ],
    },
    Topic {
        key: "society",
        name: "Society and politics",
        default: false,
        statements: &[
            "All drugs should be legalised.",
            "Churches should not be exempt from tax.",
            "Technology is making humans less empathetic.",
            "Life was better before industrialisation.",
            "Economic growth is still the best way to reduce suffering.",
            "Democracy is the best form of government we know of.",
            "Borders should be much more open than they are.",
            "Social media has been net negative for society.",
            "Nuclear power should be a large part of Germany's energy mix.",
            "% What is the chance that global warming stays below 2°C?",
            "Inheritance should be taxed much more heavily.",
            "Universities should teach effective altruism as a subject.",
            "Voting is a moral duty.",
            "Capitalism is the main cause of the climate crisis.",
            "Humans can effectively mitigate climate change.",
        ],
    },
    Topic {
        key: "future",
        name: "The future",
        default: false,
        statements: &[
            "% What is the chance that humanity goes extinct before 2100?",
            "Humanity will settle other star systems.",
            "Ageing should be treated as a disease to be cured.",
            "Most people alive today will live to see radical life extension.",
            "The world will be better in 50 years than it is now.",
            "Genetic enhancement of humans is desirable.",
            "Engineered pandemics are a bigger risk than AI.",
            "Cryonics is a reasonable bet.",
            "% What is the chance that cryonics would work for someone frozen today?",
            "% What is the chance that humans walk on Mars by 2040?",
            "Humanity's long-term future is more important than its present.",
            "Space colonisation is a distraction from problems on Earth.",
        ],
    },
    Topic {
        key: "life",
        name: "Warm-up",
        default: false,
        statements: &[
            "Pineapple belongs on pizza.",
            "Tabs are better than spaces.",
            "Remote work is better than working in an office.",
            "Most meetings should be emails.",
            "Reading fiction makes people better.",
            "A hot dog is a sandwich.",
            "Morning people are more productive.",
            "Everyone should learn to code.",
            "Having children is the most meaningful thing most people do.",
            "Munich is the best city in Germany to live in.",
        ],
    },
];

/// The topic with this key.
pub fn topic(key: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| t.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_and_statements_are_unique_and_short_enough() {
        let mut keys = HashSet::new();
        let mut seen = HashSet::new();
        for t in TOPICS {
            assert!(keys.insert(t.key), "{} twice", t.key);
            assert!(t.statements.len() >= 10, "{} is thin", t.key);
            for s in t.statements {
                assert!(seen.insert(*s), "{s} twice");
                assert!(s.chars().count() <= crate::game::MAX_STATEMENT, "{s}");
            }
        }
        assert!(seen.len() >= 120);
        for s in seen.iter().filter_map(|s| s.strip_prefix("% ")) {
            assert!(
                s.starts_with("What is the chance that ") && s.ends_with('?'),
                "{s}"
            );
        }
    }
}
