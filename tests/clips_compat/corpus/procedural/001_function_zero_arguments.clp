;; A zero-argument deffunction returns its last expression.
;; Level: boundary
;; Covers: answer, deffunction
;; Run with load, reset, and run in a fresh environment.

(deffunction answer () 42)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (answer) crlf))
