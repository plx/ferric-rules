;; While skips its body when the initial condition is false.
;; Level: boundary
;; Covers: while
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (while FALSE do (printout t "wrong" crlf))
    (printout t "after" crlf))
