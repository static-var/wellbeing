use crate::{database::ProfileRecord, tenant::TenantRuntime};

const CRISIS_RESPONSE: &str = "I’m really glad you said that out loud. I’m not equipped to handle crisis situations, but I want to take this seriously. If you might act on these feelings or you’re in immediate danger, please contact local emergency services now or reach out to a crisis line in your area right away. If you want, stay here and send one short message about what country you’re in or whether someone nearby can be with you.";

const OFF_SCOPE_RESPONSE: &str = "I’m here as a supportive buddy for emotional check-ins and personal reflection, not for work, coding, homework, deployment, or project help. If you want, you can tell me what’s weighing on you personally and we can stay with that instead.";
const CLINICAL_RESPONSE: &str = "I can listen and help you reflect, but I’m not a therapist or clinician and I shouldn’t diagnose, assess, or prescribe. If you want, tell me what you’re feeling in your own words and we can stay grounded with that.";

pub enum GuardrailDecision {
    Allow,
    Reply(String),
}

pub fn evaluate_user_message(input: &str) -> GuardrailDecision {
    let normalized = input.to_lowercase();

    let crisis_terms = [
        "kill myself",
        "suicide",
        "self harm",
        "hurt myself",
        "end my life",
        "i want to die",
    ];
    if crisis_terms.iter().any(|term| normalized.contains(term)) {
        return GuardrailDecision::Reply(CRISIS_RESPONSE.to_string());
    }

    let off_scope_terms = [
        "docker",
        "kubernetes",
        "deploy",
        "deployment",
        "server",
        "api",
        "homework",
        "assignment",
        "project",
        "code",
        "coding",
        "programming",
        "resume",
        "interview prep",
        "spreadsheet",
        "marketing copy",
        "sql query",
    ];
    if off_scope_terms
        .iter()
        .any(|term| normalized.contains(term))
    {
        return GuardrailDecision::Reply(OFF_SCOPE_RESPONSE.to_string());
    }

    let clinical_terms = [
        "diagnose me",
        "what disorder",
        "am i bipolar",
        "am i depressed",
        "prescribe",
        "medication",
        "dose",
        "therapist",
        "psychiatrist",
    ];
    if clinical_terms.iter().any(|term| normalized.contains(term)) {
        return GuardrailDecision::Reply(CLINICAL_RESPONSE.to_string());
    }

    GuardrailDecision::Allow
}

pub fn sanitize_assistant_reply(reply: String) -> String {
    let normalized = reply.to_lowercase();
    if normalized.contains("```")
        || normalized.contains("docker")
        || normalized.contains("kubernetes")
        || normalized.contains("kubectl")
        || normalized.contains("terraform")
        || normalized.contains("sql")
        || normalized.contains("homework")
        || normalized.contains("assignment")
    {
        return OFF_SCOPE_RESPONSE.to_string();
    }

    if normalized.contains("diagnosis")
        || normalized.contains("diagnose")
        || normalized.contains("medication")
        || normalized.contains("dosage")
        || normalized.contains("prescribe")
    {
        return CLINICAL_RESPONSE.to_string();
    }

    reply
}

pub fn system_prompt(
    tenant: &TenantRuntime,
    profile: &ProfileRecord,
    latest_memory_summary: Option<&str>,
) -> String {
    let user_name = profile
        .user_name
        .as_deref()
        .unwrap_or("the user");
    let support_goals = profile
        .support_goals
        .as_deref()
        .unwrap_or("Offer grounded, kind, non-judgmental companionship.");
    let preferred_style = profile
        .preferred_style
        .as_deref()
        .unwrap_or("gentle, concise, warm, and human");
    let user_context = profile
        .user_context
        .as_deref()
        .unwrap_or("No additional personal background has been provided yet.");
    let memory_summary = latest_memory_summary.unwrap_or(
        "No durable relationship memory has been recorded yet. Stay attentive to stable preferences and boundaries.",
    );

    format!(
        "{persona}\n\n{bootstrap}\n\nProduct guardrails:\n- You are a supportive buddy and emotional companion.\n- You are not a therapist, psychiatrist, doctor, lawyer, teacher, coder, employee assistant, or homework helper.\n- Refuse requests for work, coding, DevOps, homework, project delivery, or professional task execution.\n- Do not provide medical, legal, or crisis instructions.\n- Do not claim to diagnose, prescribe, or replace a clinician.\n- Keep the tone calm, grounding, warm, and low-pressure.\n- Prefer reflection, validation, and gentle next steps.\n\nUser-specific context:\n- Preferred companion name: {companion_name}\n- User name: {user_name}\n- Support goals: {support_goals}\n- Preferred style: {preferred_style}\n- User context: {user_context}\n- Relationship memory summary: {memory_summary}",
        persona = tenant.persona,
        bootstrap = tenant.bootstrap,
        companion_name = profile.companion_name,
        user_name = user_name,
        support_goals = support_goals,
        preferred_style = preferred_style,
        user_context = user_context,
        memory_summary = memory_summary
    )
}
