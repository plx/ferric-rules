;; format reads at most 75 bytes of a directive, as CLIPS's buffer does: a
;; longer one is literal text and takes no argument.
;; Level: boundary
;; Covers: format
(defrule probe =>
  (printout t (format nil "[%000000000000000000000000000000000000000000000000000000000000000000000000000000005d]") crlf)
  (printout t (format nil "[%00000000000000000000000000000000000000000000000000000000000000000000005d]" 7) crlf))
