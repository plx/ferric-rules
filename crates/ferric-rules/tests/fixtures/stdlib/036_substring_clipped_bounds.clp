;; Substring clips low and high indices and returns empty for reversed bounds.
;; Level: boundary
;; Covers: sub-string
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t "[" (sub-string 0 2 "abc") "] [" (sub-string 2 9 "abc") "] [" (sub-string 3 2 "abc") "]" crlf))
