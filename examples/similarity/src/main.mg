// Semantic search: embed a corpus, rank it against a query.
// First real mongoose program.
import py "sentence_transformers"

struct Hit {
    text str
    score float
}

fn rank(query str, corpus list[str]) (list[Hit], error?) {
    model := check sentence_transformers.SentenceTransformer("all-MiniLM-L6-v2")
    q := check model.encode([query])
    c := check model.encode(corpus)
    sims := check sentence_transformers.util.cos_sim(q, c)
    scores := check list[float](sims[0].tolist())
    hits := range(len(corpus)).map(fn(i) { Hit{text: corpus[i], score: scores[i]} })
    return hits.sorted_by(fn(a, b) { a.score > b.score }), none
}

fn main() (error?) {
    corpus := [
        "The mongoose hunts snakes at dawn.",
        "Pip resolved my dependencies into a broken mess again.",
        "The kernel panicked during the ARM64 boot sequence.",
        "She trains transformers on a single consumer GPU.",
        "Fresh bread needs only flour, water, salt, and time.",
        "Rikki-Tikki-Tavi kept the bungalow safe from cobras.",
    ]
    query := "an animal that kills cobras"
    hits := check rank(query, corpus)
    printf("query: %s\n\n", query)
    for h in hits {
        printf("%.3f  %s\n", h.score, h.text)
    }
    return none
}
