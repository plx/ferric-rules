; Advanced multifield operations: create$, length$, nth$, member$.
; Uses inline expressions since bind only works for globals in Ferric.
(deffacts startup (run-mf))

(defrule mf-ops
    (run-mf)
    =>
    (printout t "len: " (length$ (create$ a b c d e)) crlf)
    (printout t "2nd: " (nth$ 2 (create$ a b c d e)) crlf)
    (printout t "pos-c: " (member$ c (create$ a b c d e)) crlf)
    (printout t "pos-z: " (member$ z (create$ a b c d e)) crlf))
