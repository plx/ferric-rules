;; RHS if executes exactly the selected branch.
;; Level: basic
;; Covers: if
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (if TRUE then (printout t "then" crlf) else (printout t "wrong" crlf))
    (if FALSE then (printout t "wrong" crlf) else (printout t "else" crlf)))
