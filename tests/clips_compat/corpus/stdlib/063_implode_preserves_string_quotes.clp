;; Implode returns a textual representation preserving quoted STRING fields.
;; Level: basic
;; Covers: create$, implode$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (implode$ (create$ a "two words" 3)) crlf))
