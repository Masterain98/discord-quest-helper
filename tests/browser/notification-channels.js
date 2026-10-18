async (page) => {
  const report = {};
  await page.goto(
    new URL("/tests/browser/notification-channels.html", page.url()).href,
  );
  await page.setViewportSize({ width: 1280, height: 900 });
  await page
    .getByRole("button", { name: "Configure Notification test", exact: true })
    .click();
  {
    const dialog = page.getByRole("dialog");
    const inputs = dialog.locator("input");
    if ((await inputs.count()) !== 4)
      throw new Error("Fourth channel fields missing");
    const types = await inputs.evaluateAll((nodes) =>
      nodes.map((node) => node.type),
    );
    if (
      JSON.stringify(types) !==
      JSON.stringify(["url", "password", "password", "text"])
    )
      throw new Error("Incorrect field types");
    if (
      (await dialog
        .getByRole("textbox", { name: "Chat ID", exact: true })
        .inputValue()) !== "18446744073709551615"
    )
      throw new Error("Long ID changed");
    await dialog
      .getByRole("textbox", { name: "Bot Token", exact: true })
      .fill("fixture-api-input");
    await dialog
      .getByRole("textbox", { name: "Saved; leave blank to keep", exact: true })
      .fill("fixture-signing-input");
    await dialog
      .getByRole("button", { name: "Test push", exact: true })
      .click();
  }
  await page.getByRole("button", { name: "Message type", exact: true }).click();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  {
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Test push", exact: true })
      .click();
    const dialog = page.getByRole("dialog");
    await dialog
      .getByRole("heading", { name: "Configure Notification test" })
      .waitFor();
    const cleared = await dialog
      .locator("input[type=password]")
      .evaluateAll((nodes) => nodes.every((node) => node.value === ""));
    if (!cleared) throw new Error("Password drafts survived test");
    const initial = await page.evaluate(() => {
      const qa = window.__notificationQA;
      const calls = qa.calls.filter(
        (c) => c.command === "test_notification_channel",
      );
      const request = calls[calls.length - 1].args;
      qa.debug.value = false;
      return { kind: request.event.kind, locale: request.event.locale };
    });
    if (initial.kind !== "quest_started" || initial.locale !== "en")
      throw new Error("Preview schema changed");
    await dialog
      .getByRole("textbox", { name: "Webhook URL", exact: true })
      .fill("https://example.invalid/edited");
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await dialog
      .getByText("Saved. Enable the channel separately when ready.")
      .waitFor();
    await page.evaluate(() => (window.__notificationQA.failSave = true));
    await dialog
      .getByRole("textbox", { name: "Webhook URL", exact: true })
      .fill("https://example.invalid/rejected");
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await dialog.getByRole("alert").waitFor();
    if (
      (await dialog
        .getByRole("textbox", { name: "Webhook URL", exact: true })
        .getAttribute("aria-invalid")) !== "true"
    )
      throw new Error("Field error missing");
    await page.evaluate(() => {
      window.__notificationQA.failSave = false;
      window.__notificationQA.testError = true;
    });
    await dialog
      .getByRole("button", { name: "Test push", exact: true })
      .click();
    await dialog
      .getByText("Check the value of this field.", { exact: true })
      .waitFor();
    if ((await dialog.locator("#notification-config-error").count()) !== 1)
      throw new Error("Duplicate error IDs");
    await page.evaluate(() => (window.__notificationQA.testError = false));
    await dialog
      .getByRole("button", { name: "Clear configuration", exact: true })
      .click();
    await dialog
      .getByText("Saved. Enable the channel separately when ready.")
      .waitFor();
    const required = await dialog
      .locator("input[type=password]")
      .evaluateAll((nodes) => nodes.every((node) => node.required));
    if (!required)
      throw new Error("Cleared password fields should be required");
    const before = await page.evaluate(
      () =>
        window.__notificationQA.calls.filter(
          (c) => c.command === "save_notification_channel",
        ).length,
    );
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    const after = await page.evaluate(
      () =>
        window.__notificationQA.calls.filter(
          (c) => c.command === "save_notification_channel",
        ).length,
    );
    if (before !== after)
      throw new Error("Required-field validation did not block submit");
    await dialog
      .getByRole("textbox", { name: "Webhook URL", exact: true })
      .fill("https://example.invalid/final");
    await dialog
      .getByRole("textbox", { name: "Bot Token", exact: true })
      .fill("fixture-api-replacement");
    await dialog
      .getByRole("textbox", { name: "Saved; leave blank to keep", exact: true })
      .fill("fixture-signing-replacement");
    await dialog
      .getByRole("textbox", { name: "Chat ID", exact: true })
      .fill("18446744073709551615");
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await dialog
      .getByText("Saved. Enable the channel separately when ready.")
      .waitFor();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await page
      .getByRole("switch", { name: "Enable Notification test", exact: true })
      .click();
    if (
      (await page
        .getByRole("switch", { name: "Enable Notification test", exact: true })
        .getAttribute("aria-checked")) !== "true"
    )
      throw new Error("Enable channel failed");
    report.behavior = {
      preview: initial,
      passwordCleanup: true,
      save: true,
      fieldErrors: true,
      clear: true,
      requiredValidation: true,
      enabled: true,
    };
  }
  {
    await page.setViewportSize({ width: 390, height: 620 });
    await page.evaluate(() => {
      document.documentElement.classList.add("dark");
      window.__notificationQA.debug.value = true;
    });
    await page
      .getByRole("button", { name: "Configure Notification test", exact: true })
      .click();
    const dialog = page.getByRole("dialog");
    const size = await dialog.evaluate((node) => {
      const r = node.getBoundingClientRect();
      return {
        left: r.left,
        right: r.right,
        top: r.top,
        bottom: r.bottom,
        width: node.clientWidth,
        scrollWidth: node.scrollWidth,
      };
    });
    if (
      size.left < 0 ||
      size.right > 391 ||
      size.top < 0 ||
      size.bottom > 621 ||
      size.scrollWidth > size.width + 1
    )
      throw new Error("Narrow form overflow");
    await dialog.evaluate(async (node) => {
      await Promise.all(
        node
          .getAnimations({ subtree: true })
          .map((animation) => animation.finished.catch(() => {})),
      );
    });
    for (let n = 0; n < 9; n++) {
      await page.keyboard.press("Tab");
      if (
        !(await dialog.evaluate((node) =>
          node.contains(document.activeElement),
        ))
      )
        throw new Error("Focus escaped dialog");
    }
    const names = [
      "Notification test",
      "Task started",
      "Task completed",
      "Queue completed",
    ];
    for (const name of names) {
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Test push", exact: true })
        .click();
      await page
        .getByRole("button", { name: "Message type", exact: true })
        .click();
      await page.getByRole("menuitemradio", { name, exact: true }).click();
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Test push", exact: true })
        .click();
      await page
        .getByRole("heading", {
          name: "Configure Notification test",
          exact: true,
        })
        .waitFor();
    }
    const previews = await page.evaluate(() =>
      window.__notificationQA.calls
        .filter((c) => c.command === "test_notification_channel")
        .slice(-4)
        .map((c) => ({
          kind: c.args.event?.kind ?? "test",
          locale: c.args.event?.locale ?? c.args.locale,
        })),
    );
    if (
      JSON.stringify(previews.map((p) => p.kind)) !==
        JSON.stringify([
          "test",
          "quest_started",
          "quest_completed",
          "queue_completed",
        ]) ||
      previews.some((p) => p.locale !== "en")
    )
      throw new Error("Preview requests do not match contract");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Cancel", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Configure Telegram", exact: true })
      .click();
    if ((await page.getByRole("dialog").locator("input").count()) !== 3)
      throw new Error("Telegram form changed");
    await page
      .getByRole("dialog")
      .getByRole("textbox", { name: "Chat ID", exact: true })
      .fill("-100123456789");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Cancel", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Configure Discord Webhook", exact: true })
      .click();
    if ((await page.getByRole("dialog").locator("input").count()) !== 2)
      throw new Error("Discord form changed");
    if (
      (await page
        .getByRole("dialog")
        .getByRole("textbox", { name: "Webhook URL", exact: true })
        .getAttribute("type")) !== "password"
    )
      throw new Error("Webhook is not secret");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Cancel", exact: true })
      .click();
    report.layout = {
      darkNarrowLayout: size,
      keyboardFocus: true,
      previews,
      existingChannelForms: true,
    };
  }
  return report;
}
