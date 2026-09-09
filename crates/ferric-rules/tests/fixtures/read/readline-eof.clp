;; Level: boundary
;; Covers: readline, EOF
;; Input is supplied verbatim from the companion .in file.
(defrule probe => (printout t (readline) crlf))
