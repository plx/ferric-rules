; Multifield operations: create$, length$, nth$, member$.
; member$ returns the 1-based position when found, FALSE otherwise.
(deffacts startup (test-mf))

(defrule mf-test
    (test-mf)
    =>
    (printout t "len: " (length$ (create$ a b c d)) crlf)
    (printout t "nth: " (nth$ 2 (create$ a b c d)) crlf)
    (printout t "member: " (member$ b (create$ a b c d)) crlf))
