import { renderMarkdown } from "./markdown";

test("a fenced C block is highlighted and kept as code", () => {
  const html = renderMarkdown("```c\nint a[3] = {1, 2, 3};\nprintf(\"%d\", a[1]);\n```");
  expect(html).toContain('<pre class="md-code"><code class="hljs language-c">');
  expect(html).toContain('<span class="hljs-type">int</span>');
  expect(html).toContain("printf");
});

test("an unknown language is shown as plain, escaped code", () => {
  const html = renderMarkdown("```weird\nif (a < b && c > d) {}\n```");
  expect(html).toContain("a &lt; b &amp;&amp; c &gt; d");
  expect(html).not.toContain("hljs-");
});

test("lists, tables and inline code render", () => {
  const html = renderMarkdown("- **one** and `x++`\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |");
  expect(html).toContain("<strong>one</strong>");
  expect(html).toContain("<code>x++</code>");
  expect(html).toContain("<table>");
});

test("HTML, scripts, images and links never get through", () => {
  const html = renderMarkdown(
    '<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">\n\n![cat](https://evil.example/cat.png)\n\n[click](javascript:alert(1))\n\n<a href="https://evil.example">x</a>',
  );
  const div = document.createElement("div");
  div.innerHTML = html;
  expect(div.querySelector("script, img, a, iframe, [onerror], [href], [src]")).toBeNull();
  // The raw HTML is shown as text instead.
  expect(div.textContent).toContain("<script>alert(1)</script>");
  expect(div.textContent).toContain("click");
});
