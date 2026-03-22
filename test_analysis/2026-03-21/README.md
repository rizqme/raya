# Test Analysis Index - 2026-03-21

## Stuck / hanging tests

- `expression_tests::test_parse_object_literal_with_spread`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/expression_tests.test_parse_object_literal_with_spread.md`
- `ir_comprehensive::objects::test_object_spread_lowers_to_field_copy`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/ir_comprehensive.objects.test_object_spread_lowers_to_field_copy.md`
- `milestone_2_9_test::test_array_destructuring_with_defaults`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/milestone_2_9_test.test_array_destructuring_with_defaults.md`
- `milestone_2_9_test::test_object_destructuring_with_defaults`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/milestone_2_9_test.test_object_destructuring_with_defaults.md`
- `milestone_2_9_test::test_object_spread`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/milestone_2_9_test.test_object_spread.md`
- `milestone_2_9_test::test_complex_object_with_all_features`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/milestone_2_9_test.test_complex_object_with_all_features.md`

## HTTP E2E failures

- `cli_http_e2e::e2e_cli_http_stress_workflow`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_stress_workflow.md`
- `cli_http_e2e::e2e_cli_http_server_readiness_smoke`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_server_readiness_smoke.md`
- `cli_http_e2e::e2e_cli_http_route_sequence_contracts`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_route_sequence_contracts.md`
- `cli_http_e2e::e2e_cli_http_diag_contract`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_diag_contract.md`
- `cli_http_e2e::e2e_cli_http_echo_and_not_found_contracts`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_echo_and_not_found_contracts.md`
- `cli_http_e2e::e2e_cli_http_health_contract_and_artifacts`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_health_contract_and_artifacts.md`
- `cli_http_e2e::e2e_cli_http_echo_method_not_allowed_contract`
  File: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/cli_http_e2e.e2e_cli_http_echo_method_not_allowed_contract.md`

## Shared takeaways

- The 6 engine failures cluster around spread syntax and destructuring defaults.
- The 7 HTTP failures share one server-side crash: `Expected Object receiver for shape method call, got UnknownGcType`.
- A single fix in each cluster may clear multiple tests at once.
