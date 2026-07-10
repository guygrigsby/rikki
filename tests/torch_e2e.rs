//! The flagship path: a real project, a real venv, a tiny CPU pretrain.
//! Heavy (downloads torch on a cold uv cache), so it only runs with
//! RIKKI_TEST_TORCH=1; the default gate stays stdlib-python only.

use std::process::Command;

const TRAIN_RK: &str = r#"import py "torch"

fn main() (error?) {
    check torch.manual_seed(0)
    x := check torch.randn([64, 1])
    y := check (x * 3.0 + 1.0)
    w := check torch.zeros([1], requires_grad: true)
    b := check torch.zeros([1], requires_grad: true)
    first := 0.0
    last := 0.0
    for i := range 200 {
        pred := check (x * w + b)
        diff := check (pred - y)
        loss := check (diff * diff).mean()
        check loss.backward()
        with torch.no_grad() {
            check w.sub_(w.grad * 0.1)
            check b.sub_(b.grad * 0.1)
            check w.grad.zero_()
            check b.grad.zero_()
        }
        l := check float(loss.item())
        if i == 0 {
            first = l
        }
        last = l
    }
    printf("first %.4f last %.6f\n", first, last)
    if last < first / 100.0 {
        print("trained")
    }
    wv := check float(w.item())
    if wv > 2.9 && wv < 3.1 {
        print("converged")
    }
    return none
}
"#;

// manual attention through the `@` operator vs the fused kernel; on CPU
// SDPA is the math backend, so this pins rikki's matmul/softmax path
// against torch's reference, not flash-vs-not (that is torch's lane)
const ATTN_RK: &str = r#"import "math"
import py "torch"

fn attention(q py, k py, v py) (py, error?) {
    scale := math.sqrt(8.0)
    w := check torch.softmax(q @ k.transpose(-2, -1) / scale, dim: -1)
    out := check (w @ v)
    return out, none
}

fn main() (error?) {
    check torch.manual_seed(0)
    q := check torch.randn([2, 4, 8])
    k := check torch.randn([2, 4, 8])
    v := check torch.randn([2, 4, 8])
    manual, err := attention(q, k, v)
    if err != none {
        return err
    }
    fused := check torch.nn.functional.scaled_dot_product_attention(q, k, v)
    same := check bool(torch.allclose(manual, fused, atol: 0.000001))
    if same {
        print("attention agrees")
    }
    return none
}
"#;

#[test]
fn tiny_cpu_pretrain() {
    if std::env::var("RIKKI_TEST_TORCH").is_err() {
        eprintln!("skipping: set RIKKI_TEST_TORCH=1 (downloads torch on a cold cache)");
        return;
    }
    let d = rikki::testutil::tempdir("torch-e2e");
    let bin = env!("CARGO_BIN_EXE_rikki");
    let out = Command::new(bin)
        .args(["new", "train"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proj = d.join("train");
    // declare torch the way a user would; py add also provisions the venv
    let out = Command::new(bin)
        .args(["py", "add", "torch"])
        .current_dir(&proj)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "rikki py add torch: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::write(proj.join("src/main.rk"), TRAIN_RK).unwrap();
    let out = Command::new(bin)
        .args(["run"])
        .current_dir(&proj)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "run failed:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("trained"),
        "loss did not collapse:\n{stdout}"
    );
    assert!(stdout.contains("converged"), "w missed 3.0:\n{stdout}");

    // same provisioned project, second program: attention both ways
    std::fs::write(proj.join("src/attn.rk"), ATTN_RK).unwrap();
    let out = Command::new(bin)
        .args(["run", "src/attn.rk"])
        .current_dir(&proj)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "attn failed:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("attention agrees"),
        "manual @ path diverged from SDPA:\n{stdout}"
    );
}
