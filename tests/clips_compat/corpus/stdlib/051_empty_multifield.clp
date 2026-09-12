;; Empty multifields have length zero and print as empty parentheses.
;; Level: boundary
;; Covers: create$, length$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (length$ (create$)) " " (create$) crlf))
