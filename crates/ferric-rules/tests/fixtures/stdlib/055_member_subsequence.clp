;; Member accepts a MULTIFIELD search value and returns its inclusive index range.
;; Level: basic
;; Covers: create$, member$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (member$ (create$ b c) (create$ a b c d)) " " (member$ (create$ b d) (create$ a b c d)) crlf))
