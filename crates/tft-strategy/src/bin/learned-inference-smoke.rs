use std::fs;

use tft_domain::{BoardPhase, GameSnapshot, ModelMetadata, UnitInstance};
use tft_strategy::{
    LearnedInferenceMode, LearnedKernel, LearnedPolicyAction, PhaseRouter, RuleKernel,
    StrategyKernel,
};

fn assert_prediction(
    kernel: &LearnedKernel,
    snapshot: &GameSnapshot,
    expected: LearnedPolicyAction,
    label: &str,
) -> Result<LearnedPolicyAction, Box<dyn std::error::Error>> {
    let action = kernel
        .predict_action(snapshot)
        .ok_or("failed to load exported ONNX policy")?;
    if action != expected {
        return Err(format!(
            "expected {label} action {}, got {}",
            expected.as_str(),
            action.as_str()
        )
        .into());
    }
    Ok(action)
}

fn rule_action_label(snapshot: &GameSnapshot) -> String {
    let plan = PhaseRouter::new(RuleKernel::default()).plan(snapshot);
    plan.actions
        .first()
        .map(|action| match action {
            tft_domain::GameAction::BuyUnit { .. } => "buy_unit",
            tft_domain::GameAction::BuyXp => "buy_xp",
            tft_domain::GameAction::Reroll => "reroll",
            tft_domain::GameAction::MoveBoard { .. } => "move_board",
            tft_domain::GameAction::ChooseAugment { .. } => "choose_augment",
            tft_domain::GameAction::Noop { .. } => "hold",
            _ => "other",
        })
        .unwrap_or("hold")
        .to_string()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metadata_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "artifacts/models/model-metadata-v2.json".to_string());
    let metadata: ModelMetadata = serde_json::from_str(&fs::read_to_string(&metadata_path)?)?;

    let kernel = LearnedKernel::new_with_mode(metadata.clone(), LearnedInferenceMode::Strict);

    let mut shop_snapshot = GameSnapshot::mock(5, 12);
    shop_snapshot.board_phase = BoardPhase::ShopEconomy;
    let shop_action = assert_prediction(
        &kernel,
        &shop_snapshot,
        LearnedPolicyAction::BuyUnit,
        "shop",
    )?;
    let rule_shop_action = rule_action_label(&shop_snapshot);

    let mut board_snapshot = GameSnapshot::mock(4, 12);
    board_snapshot.board_phase = BoardPhase::BoardPlacement;
    board_snapshot.bench.push(UnitInstance {
        id: "bench-smoke-1".into(),
        name: "Lux".into(),
        cost: 3,
        stars: 1,
        traits: vec!["Sorcerer".into()],
        items: vec![],
        position: None,
        kind: tft_domain::UnitKind::Unit,
    });
    let board_action = assert_prediction(
        &kernel,
        &board_snapshot,
        LearnedPolicyAction::MoveBoard,
        "board",
    )?;
    let rule_board_action = rule_action_label(&board_snapshot);

    let mut augment_snapshot = GameSnapshot::mock(6, 12);
    augment_snapshot.board_phase = BoardPhase::Augment;
    let augment_action = assert_prediction(
        &kernel,
        &augment_snapshot,
        LearnedPolicyAction::ChooseAugment,
        "augment",
    )?;
    let rule_augment_action = rule_action_label(&augment_snapshot);

    println!(
        "{}",
        serde_json::json!({
            "modelId": metadata.id,
            "modelVersion": metadata.version,
            "onnxPath": metadata.onnx_path,
            "inferenceMode": "strict",
            "predictedShopAction": shop_action.as_str(),
            "ruleShopAction": rule_shop_action,
            "predictedBoardAction": board_action.as_str(),
            "ruleBoardAction": rule_board_action,
            "predictedAugmentAction": augment_action.as_str()
            ,"ruleAugmentAction": rule_augment_action
        })
    );
    Ok(())
}
