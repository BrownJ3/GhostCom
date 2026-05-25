pub const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="description" content="Connecting">
  <title>Connecting</title>
  <style>
    :root {
      color-scheme: dark;
      --green: #29ff74;
    }

    * {
      box-sizing: border-box;
    }

    html,
    body {
      min-height: 100%;
      margin: 0;
      background: #000;
    }

    body {
      color: var(--green);
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
      letter-spacing: 0;
    }

    main {
      min-height: 100vh;
      display: flex;
      align-items: flex-start;
      padding: clamp(18px, 4vw, 32px);
    }

    .status {
      display: inline-flex;
      align-items: baseline;
      min-height: 1.5em;
      font-size: clamp(0.92rem, 2.5vw, 1.08rem);
      line-height: 1.4;
      text-shadow: 0 0 12px rgba(41, 255, 116, 0.42);
    }

    .dots span {
      animation: dot-pulse 1.2s infinite;
    }

    .dots span:nth-child(2) {
      animation-delay: 0.2s;
    }

    .dots span:nth-child(3) {
      animation-delay: 0.4s;
    }

    @keyframes dot-pulse {
      0%, 20% {
        opacity: 0.65;
      }
      45%, 100% {
        opacity: 1;
      }
    }
  </style>
</head>
<body>
  <main>
    <div class="status" role="status" aria-live="polite">Connecting<span class="dots"><span>.</span><span>.</span><span>.</span></span></div>
  </main>
</body>
</html>
"#;
