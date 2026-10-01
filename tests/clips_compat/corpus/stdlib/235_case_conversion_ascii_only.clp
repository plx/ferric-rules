;; upcase and lowcase change ASCII letters only; other characters keep their
;; case, in strings, symbols and instance names.
;; Level: boundary
;; Covers: upcase, lowcase, instance-name
(defrule probe =>
  (printout t (upcase "éaß") " " (lowcase "ÀB") crlf)
  (printout t (upcase ñz) " " (lowcase ÑZ) crlf)
  (printout t (upcase [éa]) " " (lowcase [ÉA]) crlf))
