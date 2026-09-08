;; String index returns the first one-based position or FALSE.
;; Level: interaction
;; Covers: str-index
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-index "ana" "banana") " " (str-index "z" "banana") crlf))
