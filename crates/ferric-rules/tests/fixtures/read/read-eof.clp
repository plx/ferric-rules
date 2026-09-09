;; Level: boundary
;; Covers: read, EOF
;; Input is supplied verbatim from the companion .in file.
(defrule probe => (printout t (read) crlf))
