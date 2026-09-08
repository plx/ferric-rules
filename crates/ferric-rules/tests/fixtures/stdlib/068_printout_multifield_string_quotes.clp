;; Printout quotes STRING elements inside a MULTIFIELD.
;; Level: boundary
;; Covers: printout, create$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (create$ "a" "two words") crlf))
