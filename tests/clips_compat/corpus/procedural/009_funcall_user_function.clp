;; Funcall dynamically resolves a user function.
;; Level: interaction
;; Covers: *, deffunction, funcall
;; Run with load, reset, and run in a fresh environment.

(deffunction twice (?x) (* ?x 2))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (funcall twice 6) crlf))
