import analyze_geography_metric_schema as audit


def test_summarize_metric_rows_computes_family_medians_and_iqr():
    rows = [
        {"family": "a", "anisotropy": 0.10, "ridge_linearity": 0.20, "incision_ratio": 0.03},
        {"family": "a", "anisotropy": 0.30, "ridge_linearity": 0.40, "incision_ratio": 0.09},
        {"family": "b", "anisotropy": 0.70, "ridge_linearity": 0.80, "incision_ratio": 0.12},
    ]
    out = audit.summarize_metric_rows(rows, metrics=("anisotropy", "ridge_linearity", "incision_ratio"))
    by_family = {row["family"]: row for row in out}
    assert by_family["a"]["count"] == 2
    assert by_family["a"]["anisotropy_median"] == 0.2
    assert by_family["a"]["anisotropy_iqr"] == 0.1
    assert by_family["a"]["incision_ratio_min"] == 0.03
    assert by_family["a"]["incision_ratio_max"] == 0.09
    assert by_family["b"]["ridge_linearity_median"] == 0.8


def test_audit_metric_key_order_is_stable():
    rows = [{"family": "a", "kernel_id": "k1", "anisotropy": 0.1}, {"family": "a", "extra": 2.0}]
    assert audit._keys(rows) == ["family", "kernel_id", "anisotropy", "extra"]


def test_audit_summary_records_anisotropy_decision():
    rows = [
        {"family": "a", "anisotropy_median": 0.1, "vrm_7px_median": 0.0},
        {"family": "b", "anisotropy_median": 0.5, "vrm_7px_median": 0.0},
    ]
    text = "\n".join(audit.audit_summary_lines(rows))
    assert "`anisotropy` is not dead" in text
    assert "`vrm_7px` implementation is effectively dead" in text
