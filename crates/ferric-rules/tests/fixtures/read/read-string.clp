;; Level: basic
;; Covers: read, STRING
;; Input is supplied verbatim from the companion .in file.
(defrule probe => (bind ?x (read)) (printout t (stringp ?x) ":" ?x crlf))
