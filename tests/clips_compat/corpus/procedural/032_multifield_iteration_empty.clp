;; Multifield iteration skips an empty sequence.
;; Level: boundary
;; Covers: create$, foreach, progn$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (progn$ (?item (create$)) (printout t "wrong" crlf))
    (foreach ?item (create$) (printout t "wrong" crlf))
    (printout t "after" crlf))
