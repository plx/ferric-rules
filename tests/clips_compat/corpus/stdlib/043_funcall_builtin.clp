;; Funcall dynamically selects a builtin from a symbol or string name.
;; Level: interaction
;; Covers: funcall
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (funcall + 1 2) " " (funcall "*" 3 4) crlf))
