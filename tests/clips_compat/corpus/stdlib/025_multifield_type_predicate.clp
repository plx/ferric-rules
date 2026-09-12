;; Empty and nonempty multifields have the same MULTIFIELD type.
;; Level: boundary
;; Covers: create$, multifieldp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (multifieldp (create$)) " " (multifieldp (create$ a)) " " (multifieldp a) crlf))
