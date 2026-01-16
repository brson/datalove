// Smooth scrolling for navigation links
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', function (e) {
        e.preventDefault();
        const target = document.querySelector(this.getAttribute('href'));
        if (target) {
            const navHeight = document.querySelector('.nav').offsetHeight;
            const targetPosition = target.offsetTop - navHeight;

            window.scrollTo({
                top: targetPosition,
                behavior: 'smooth'
            });
        }
    });
});

// Add scroll effect to nav
let lastScroll = 0;
const nav = document.querySelector('.nav');

window.addEventListener('scroll', () => {
    const currentScroll = window.pageYOffset;

    if (currentScroll > 100) {
        nav.style.boxShadow = '0 4px 12px rgba(0, 0, 0, 0.15)';
    } else {
        nav.style.boxShadow = '0 2px 8px rgba(0, 0, 0, 0.08)';
    }

    lastScroll = currentScroll;
});

// Intersection Observer for fade-in animations
const observerOptions = {
    threshold: 0.1,
    rootMargin: '0px 0px -50px 0px'
};

const observer = new IntersectionObserver((entries) => {
    entries.forEach(entry => {
        if (entry.isIntersecting) {
            entry.target.style.opacity = '1';
            entry.target.style.transform = 'translateY(0)';
        }
    });
}, observerOptions);

// Apply fade-in to elements
document.addEventListener('DOMContentLoaded', () => {
    const elementsToAnimate = document.querySelectorAll(
        '.feature-card, .component-block, .example-block, .install-block, .quick-start'
    );

    elementsToAnimate.forEach(el => {
        el.style.opacity = '0';
        el.style.transform = 'translateY(20px)';
        el.style.transition = 'opacity 0.6s ease, transform 0.6s ease';
        observer.observe(el);
    });
});

// Add parallax effect to geometric shapes
window.addEventListener('scroll', () => {
    const scrolled = window.pageYOffset;
    const shapes = document.querySelectorAll('.geometric-shape');

    shapes.forEach((shape, index) => {
        const speed = 0.1 + (index * 0.05);
        const yPos = -(scrolled * speed);
        shape.style.transform = `translateY(${yPos}px)`;
    });
});

// Syntax highlighting for Datalove code blocks
document.addEventListener('DOMContentLoaded', () => {
    const codeBlocks = document.querySelectorAll('pre code');

    const keywords = /\b(fun|proc|end|ret|let|if|then|else|match|enum|set|ok|open)\b/;
    const types = /\b(u8|u16|u32|u64|i8|i16|i32|i64|f32|f64|bool|string|Config|AddressEntry)\b/;
    const constructors = /\b(Friend|Family|Some|None|Ok|Err)\b/;

    codeBlocks.forEach(block => {
        const text = block.textContent;
        const tokens = [];
        let i = 0;

        while (i < text.length) {
            if (text[i] === '/' && text[i+1] === '/') {
                const start = i;
                while (i < text.length && text[i] !== '\n') i++;
                tokens.push({ type: 'comment', text: text.slice(start, i) });
                continue;
            }

            // Strings
            if (text[i] === '"') {
                const start = i;
                i++;
                while (i < text.length && text[i] !== '"') {
                    if (text[i] === '\\') i++;
                    i++;
                }
                i++;
                tokens.push({ type: 'string', text: text.slice(start, i) });
                continue;
            }

            // Numbers
            if (/\d/.test(text[i])) {
                const start = i;
                while (i < text.length && /[\d.]/.test(text[i])) i++;
                tokens.push({ type: 'number', text: text.slice(start, i) });
                continue;
            }

            // Identifiers and keywords
            if (/[a-zA-Z_]/.test(text[i])) {
                const start = i;
                while (i < text.length && /[a-zA-Z0-9_]/.test(text[i])) i++;
                const word = text.slice(start, i);

                if (keywords.test(word)) {
                    tokens.push({ type: 'keyword', text: word });
                } else if (types.test(word)) {
                    tokens.push({ type: 'type', text: word });
                } else if (constructors.test(word)) {
                    tokens.push({ type: 'constructor', text: word });
                } else {
                    tokens.push({ type: 'text', text: word });
                }
                continue;
            }

            // Operators
            if (text[i] === '+' && text[i+1] === '?') {
                tokens.push({ type: 'operator', text: '+?' });
                i += 2;
                continue;
            }

            // Punctuation
            if (/[{}()\[\]=:,.]/.test(text[i])) {
                tokens.push({ type: 'punct', text: text[i] });
                i++;
                continue;
            }

            // Whitespace and other
            tokens.push({ type: 'text', text: text[i] });
            i++;
        }

        // Build highlighted HTML
        let html = '';
        for (const token of tokens) {
            const escaped = token.text
                .replace(/&/g, '&amp;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;');

            if (token.type === 'text') {
                html += escaped;
            } else {
                html += `<span class="hl-${token.type}">${escaped}</span>`;
            }
        }

        block.innerHTML = html;
    });
});
