;; Subsequence uses inclusive one-based indices and clips outside bounds.
;; Level: boundary
;; Covers: create$, subseq$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (subseq$ (create$ a b c d) 2 3) " " (subseq$ (create$ a b) 0 9) " " (subseq$ (create$ a b) 2 1) crlf))
