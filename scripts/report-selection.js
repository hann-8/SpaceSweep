(() => {
    const toolbar = document.querySelector("#review-selection");
    if (!toolbar) return;

    const status = document.querySelector("#selection-status");
    const submit = document.querySelector("#submit-selection");
    const choices = Array.from(document.querySelectorAll("[data-review-choice]"));
    const branches = Array.from(document.querySelectorAll("[data-review-branch]"), (input) => ({
        input,
        choices: Array.from(input.closest("details").querySelectorAll("[data-review-choice]")),
    }));
    let locked = false;

    const selected = () => choices.filter((input) => input.checked);

    function refresh() {
        for (const branch of branches) {
            const count = branch.choices.filter((input) => input.checked).length;
            branch.input.checked = count > 0 && count === branch.choices.length;
            branch.input.indeterminate = count > 0 && count < branch.choices.length;
            branch.input.disabled = locked || branch.choices.length === 0;
        }
        for (const input of choices) input.disabled = locked;
        const checked = selected();
        const yellow = checked.filter((input) => input.dataset.reviewLevel === "needs_review").length;
        submit.disabled = locked || checked.length === 0;
        status.textContent = `已勾选黄色 ${yellow} 项 · 绿色 ${checked.length - yellow} 项`;
    }

    for (const branch of branches) {
        branch.input.addEventListener("change", () => {
            for (const input of branch.choices) input.checked = branch.input.checked;
            refresh();
        });
    }
    for (const input of choices) input.addEventListener("change", refresh);

    submit.addEventListener("click", async () => {
        const checked = selected();
        if (locked || !checked.length) return;
        locked = true;
        refresh();
        status.textContent = "正在提交勾选…";
        try {
            const response = await fetch("selection", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ choices: checked.map((input) => Number(input.dataset.reviewChoice)) }),
            });
            if (!response.ok) throw new Error("Review submission failed");
            status.textContent = "勾选已提交。回到当前对话查看结果。";
        } catch {
            locked = false;
            refresh();
            status.textContent = "提交失败。请重试，或回到对话重新打开审核链接。";
        }
    });
    refresh();
})();
