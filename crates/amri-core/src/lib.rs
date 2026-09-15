pub mod health;
pub mod i18n;
pub mod hot_pool;
pub mod model;
pub mod route_proof;
pub mod scoring;
pub mod selector;
pub mod shadow_race;

pub use i18n::{text as ui_text, Language, UiMessage};
pub use health::{CircuitBreakerPolicy, RouteHealthState, RouteHealthTracker};
pub use hot_pool::{build_hot_pool, HotPoolEntry, HotPoolPolicy};
pub use model::{
    DestinationKey, NetworkProfile, NodeId, ProbeSample, RouteCandidate, TrafficClass,
};
pub use route_proof::{CandidateEvidence, RouteProof, RouteProofChain, RouteProofError};
pub use scoring::{ScoreBreakdown, ScoringProfile};
pub use selector::{
    RouteDecision, RouteSelector, RouteTransitionAction, RouteTransitionDecision, SelectionPolicy,
};
pub use shadow_race::{ShadowRace, ShadowRaceOutcome, ShadowRacePolicy, ShadowRaceSample};
