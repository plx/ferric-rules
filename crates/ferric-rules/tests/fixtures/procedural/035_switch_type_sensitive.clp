;; Switch case matching distinguishes integer and float types.
;; Level: boundary
;; Covers: case, switch
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (switch 2.0
        (case 2 then (printout t "wrong" crlf))
        (case 2.0 then (printout t "float" crlf))))
