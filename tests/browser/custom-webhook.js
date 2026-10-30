async (page) => {
  const check = (value, message) => { if (!value) throw new Error(message); };
  await page.goto(new URL("/tests/browser/notification-channels.html", page.url()).href);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.getByRole("button", { name: "Configure Custom Webhook", exact: true }).click();
  await page.evaluate(() => { window.__notificationQA.debug.value = false; });
  const dialog = () => page.getByRole("dialog");
  const title = () => dialog().locator("input[id$='_title']");
  const content = () => dialog().locator("textarea");
  const event = () => dialog().locator("#notification-template-event");
  const save = async (name = "Save") => {
    await dialog().getByRole("button", { name, exact: true }).click();
    await dialog().getByRole("status").filter({ hasText: /Saved|已保存/ }).waitFor();
  };
  check(await content().inputValue() === "{quest-name}", "Default template was interpolated");
  await dialog().getByRole("textbox", { name: "Webhook Endpoint", exact: true }).fill("http://127.0.0.1:8787/hook");
  await dialog().getByRole("textbox", { name: "Bearer Token (optional)", exact: true }).fill("fixture-token");
  await save();
  const first = await page.evaluate(() => window.__notificationQA.calls.filter(c => c.command === "save_notification_channel").at(-1).args.draft);
  check(!Object.keys(first.fields).some(key => key.endsWith("_content") || key.endsWith("_title")), "Saving materialized default templates");
  check(await dialog().locator("input[type=password]").evaluateAll(nodes => nodes.every(node => node.value === "")), "Saved secrets remained visible");

  const custom = "  自定义\n{quest-name}\n{my-text}  ";
  await content().fill(custom);
  await dialog().getByText("Unknown variables will be sent unchanged: {my-text}", { exact: true }).waitFor();
  await event().focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Home");
  await page.keyboard.press("Enter");
  check((await content().inputValue()).includes("{estimated-complete-at}"), "Start default missing");
  await event().click();
  check(await page.getByRole("menuitemradio", { name: "Task started", exact: true }).getAttribute("aria-checked") === "true", "Current event is not marked");
  await page.getByRole("menuitemradio", { name: "Task completed", exact: true }).click();
  check(await content().inputValue() === custom, "Switching events lost the draft");
  await save();
  await page.evaluate(() => { window.__notificationQA.locale.value = "zh"; });
  check(await title().inputValue() === "任务已完成", "Default title did not follow language");
  check(await content().inputValue() === custom, "Language switch overwrote custom content");

  await content().fill("");
  await save("保存");
  const empty = await page.evaluate(() => window.__notificationQA.settings.channels.find(c => c.id === "custom_webhook").values.quest_completed_content);
  check(empty === "", "Empty override did not persist");
  await content().locator("..").getByRole("button", { name: "恢复默认", exact: true }).click();
  check(await content().inputValue() === "{quest-name}", "Restore default failed");
  await save("保存");
  check(await page.evaluate(() => !Object.hasOwn(window.__notificationQA.settings.channels.find(c => c.id === "custom_webhook").values, "quest_completed_content")), "Restore saved an empty override");
  await dialog().getByRole("button", { name: "移除 Token", exact: true }).click();

  // The ordinary-mode test chooser retains the current reset and all template drafts.
  await content().fill("草稿 {quest-name}");
  const savesBeforeTests = await page.evaluate(() => window.__notificationQA.calls.filter(c => c.command === "save_notification_channel").length);
  await page.evaluate(() => { window.__notificationQA.testCode = "submitted"; });
  for (const name of ["任务开始", "任务完成", "队列完成"]) {
    await dialog().getByRole("button", { name: "测试推送", exact: true }).click();
    await page.getByRole("button", { name: "消息类型", exact: true }).click();
    await page.getByRole("menuitemradio", { name, exact: true }).click();
    await dialog().getByRole("button", { name: "测试推送", exact: true }).click();
    await page.getByRole("heading", { name: "配置 自定义 Webhook", exact: true }).waitFor();
    await dialog().getByText("接收端已接受", { exact: true }).waitFor();
  }
  const tests = await page.evaluate(() => {
    const qa = window.__notificationQA;
    return {
      requests: qa.calls.filter(c => c.command === "test_notification_channel").map(c => c.args),
      saves: qa.calls.filter(c => c.command === "save_notification_channel").length,
      tokenSaved: qa.settings.channels.find(c => c.id === "custom_webhook").secret_fields_set.includes("bearer_token"),
    };
  });
  check(tests.saves === savesBeforeTests && tests.tokenSaved, "Tests changed saved configuration");
  check(JSON.stringify(tests.requests.map(r => r.event.kind)) === JSON.stringify(["quest_started", "quest_completed", "queue_completed"]), "Ordinary-mode event tests missing");
  check(tests.requests.every(r => r.draft.reset_fields.includes("bearer_token") && !Object.hasOwn(r.draft.fields, "bearer_token")), "Token reset conflicted with password cleanup");
  check(tests.requests.every(r => r.draft.fields.quest_completed_content === "草稿 {quest-name}"), "Tests ignored the unsaved template");
  await save("保存");
  check(await page.evaluate(() => !window.__notificationQA.settings.channels.find(c => c.id === "custom_webhook").secret_fields_set.includes("bearer_token")), "Token removal failed");
  await dialog().getByRole("button", { name: "取消", exact: true }).click();
  await page.getByRole("button", { name: "配置 自定义 Webhook", exact: true }).click();
  check(await content().inputValue() === "草稿 {quest-name}", "Saved customization did not survive reopening");

  await dialog().getByText("可用变量", { exact: true }).click();
  for (const dark of [false, true]) {
    await page.evaluate(value => document.documentElement.classList.toggle("dark", value), dark);
    await page.setViewportSize({ width: 390, height: 620 });
    const bounds = await dialog().evaluate(async node => {
      await Promise.all(node.getAnimations({ subtree: true }).map(a => a.finished.catch(() => {})));
      const r = node.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: node.clientWidth, scrollWidth: node.scrollWidth };
    });
    check(bounds.left >= 0 && bounds.right <= 391 && bounds.top >= 0 && bounds.bottom <= 621 && bounds.scrollWidth <= bounds.width + 1, "Narrow template form overflows");
    await page.keyboard.press("Tab");
    check(await dialog().evaluate(node => node.contains(document.activeElement)), "Keyboard focus escaped");
    await dialog().evaluate(async node => {
      node.scrollTop = 0;
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await Promise.all(node.getAnimations({ subtree: true }).map(a => a.finished.catch(() => {})));
    });
    await page.screenshot({ path: dark ? "output/playwright/custom-webhook-dark-narrow.png" : "output/playwright/custom-webhook-light-narrow.png" });
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await dialog().evaluate(async node => {
    node.scrollTop = 0;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.all(node.getAnimations({ subtree: true }).map(a => a.finished.catch(() => {})));
  });
  await page.screenshot({ path: "output/playwright/custom-webhook-light.png" });
  return { sparseDefaults: true, localeSwitch: true, emptyOverride: true, restoreDefault: true, secretRemoval: true, ordinaryModeTests: tests.requests.map(r => r.event.kind), layout: "light/dark narrow and desktop", draftPersistence: true };
}
