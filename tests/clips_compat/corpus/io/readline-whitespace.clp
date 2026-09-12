;; Level: boundary
;; Covers: readline, whitespace
;; Input is supplied verbatim from the companion .in file.
(defrule probe => (printout t "[" (readline) "]" crlf))
