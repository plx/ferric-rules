;; Level: boundary
;; Covers: read, tokenization
;; Input is supplied verbatim from the companion .in file.
(defrule probe => (printout t (read) ":" (read) crlf))
