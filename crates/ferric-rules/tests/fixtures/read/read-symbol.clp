;; Level: basic
;; Covers: read, SYMBOL
;; Input is supplied verbatim from the companion .in file.
(defrule probe => (bind ?x (read)) (printout t (symbolp ?x) ":" ?x crlf))
