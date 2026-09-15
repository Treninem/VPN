pub mod health;
pub mod hot_pool;
pub mod i18n;
pub mod mobile;
pub mod model;
pub mod protection;
pub mod route_proof;
pub mod scoring;
pub mod selector;
pub mod shadow_race;

pub use health::{CircuitBreakerPolicy, RouteHealthState, RouteHealthTracker};
pub use hot_pool::{build_hot_pool, HotPoolEntry, HotPoolPolicy};
pub use i18n::{text as ui_text, Language, UiMessage};
pub use mobile::{
    AccessNetworkKind, AdaptiveMtuController, AdaptiveMtuError, AdaptiveMtuPolicy,
    MobileAccelerationMode, MobileAccelerationPreferences, MobileNetworkSnapshot, MobilePathPolicy,
    MobileRuntimeBudget, ProbeIntensity,
};
pub use model::{
    DestinationKey, NetworkProfile, NodeId, ProbeSample, RouteCandidate, TrafficClass,
};
pub use protection::{
    evaluate_protection, ProtectionReadiness, ProtectionRequirement, ProtectionSignals,
    ProtectionState,
};
pub use route_proof::{CandidateEvidence, RouteProof, RouteProofChain, RouteProofError};
pub use scoring::{score_candidate, ScoreBreakdown, ScoringProfile};
pub use selector::{
    RouteDecision, RouteSelector, RouteTransitionAction, RouteTransitionDecision, SelectionPolicy,
};
pub use shadow_race::{ShadowRace, ShadowRaceOutcome, ShadowRacePolicy, ShadowRaceSample};
