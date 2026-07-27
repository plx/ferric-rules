;; Phase 4: Multifield function surface fixture.
;; Exercises create$, length$, nth$, member$, subsetp.

(defrule test-multifield
    (go)
    =>
    (printout t "create$: " (create$ 1 2 3) crlf)
    (printout t "length$: " (length$ (create$ a b c d)) crlf)
    (printout t "nth$: " (nth$ 2 (create$ a b c)) crlf)
    (printout t "member$: " (member$ b (create$ a b c)) crlf)
    (printout t "subsetp: " (subsetp (create$ 1 2) (create$ 1 2 3)) crlf))

(deffacts startup (go))
