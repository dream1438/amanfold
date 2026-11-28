// amanfold.com behaviour. No cookies, no storage, no third parties.
// Talks only to the waitlist endpoints of the Amanfold server (docs/SERVER.md).
(function () {
  "use strict";

  var meta = document.querySelector('meta[name="af-api"]');
  var API = (meta && meta.content) || "https://api.amanfold.com";

  function post(path, body) {
    return fetch(API + path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      credentials: "omit",
      cache: "no-store",
      referrerPolicy: "no-referrer",
    }).then(
      function (r) { return { status: r.status }; },
      function () { return { status: 0 }; } // network error, blocked, offline
    );
  }

  // ---- mobile menu --------------------------------------------------------
  var menuBtn = document.querySelector(".menu-btn");
  var nav = document.getElementById("site-nav");
  if (menuBtn && nav) {
    menuBtn.addEventListener("click", function () {
      var open = nav.classList.toggle("open");
      menuBtn.setAttribute("aria-expanded", open ? "true" : "false");
    });
    nav.addEventListener("click", function (e) {
      if (e.target.closest("a")) {
        nav.classList.remove("open");
        menuBtn.setAttribute("aria-expanded", "false");
      }
    });
  }

  // ---- "Join the beta" buttons on plan cards preselect the plan ----------
  document.querySelectorAll("[data-plan]").forEach(function (a) {
    a.addEventListener("click", function () {
      var r = document.querySelector('input[name="plan"][value="' + a.getAttribute("data-plan") + '"]');
      if (r) r.checked = true;
      var email = document.getElementById("join-email");
      if (email) setTimeout(function () { email.focus({ preventScroll: true }); }, 400);
    });
  });

  // ---- waitlist form --------------------------------------------------------
  var form = document.getElementById("join-form");
  if (form) {
    var input = document.getElementById("join-email");
    var err = document.getElementById("join-error");
    var btn = form.querySelector('button[type="submit"]');
    var done = document.getElementById("join-done");
    var msg = function (k) { return form.getAttribute("data-msg-" + k) || ""; };
    var show = function (text) {
      err.textContent = text;
      err.hidden = !text;
      input.setAttribute("aria-invalid", text ? "true" : "false");
    };
    var looksLikeEmail = function (v) { return /^[^\s@]+@[^\s@]+\.[^\s@.]+$/.test(v) && v.length <= 254; };

    input.addEventListener("input", function () { if (!err.hidden) show(""); });

    form.addEventListener("submit", function (e) {
      e.preventDefault();
      var email = input.value.trim();
      if (!looksLikeEmail(email)) { show(msg("err-email")); input.focus(); return; }
      show("");
      var checked = form.querySelector('input[name="plan"]:checked');
      btn.disabled = true;
      btn.textContent = msg("sending");
      post("/v1/waitlist", { email: email, lang: form.getAttribute("data-lang"), plan: checked ? checked.value : "free" })
        .then(function (r) {
          btn.disabled = false;
          btn.textContent = msg("submit");
          if (r.status === 202 || r.status === 200) {
            done.querySelector("[data-email]").textContent = email;
            form.hidden = true;
            done.hidden = false;
            done.focus();
          } else if (r.status === 400) {
            show(msg("err-email")); input.focus();
          } else if (r.status === 429) {
            show(msg("err-rate"));
          } else if (r.status === 0) {
            show(msg("err-net"));
          } else {
            show(msg("err-other"));
          }
        });
    });
  }

  // ---- confirm / leave pages ------------------------------------------------
  var box = document.getElementById("status");
  if (box) {
    // The token travels in the #fragment so it never reaches any web server's
    // logs. Read it, then remove it from the address bar and history.
    var m = /(?:^#|&)t=([A-Za-z0-9_-]{20,64})/.exec(location.hash);
    var token = m ? m[1] : null;
    if (token && history.replaceState) history.replaceState(null, "", location.pathname + location.search);

    var setState = function (name) {
      box.querySelectorAll("[data-state]").forEach(function (s) { s.hidden = s.getAttribute("data-state") !== name; });
      var h = box.querySelector('[data-state="' + name + '"] h1');
      if (h) h.focus();
    };
    var action = box.getAttribute("data-action");

    if (!token) {
      setState("missing");
    } else if (action === "confirm") {
      post("/v1/waitlist/confirm", { token: token }).then(function (r) {
        if (r.status === 200) setState("ok");
        else if (r.status === 401 || r.status === 400) setState("bad");
        else setState("error");
      });
    } else if (action === "leave") {
      var leaveBtn = box.querySelector("[data-leave]");
      leaveBtn.addEventListener("click", function () {
        setState("working");
        post("/v1/waitlist/leave", { token: token }).then(function (r) {
          setState(r.status === 200 ? "ok" : "error");
        });
      });
    }
  }
})();
