;; Source integer overflow clamps to signed 64-bit bounds during load.
;; Level: boundary
;; Covers: source-scanner, deffacts, integerp, create$
(deffacts seed
  (bounds 9223372036854775807 9223372036854775808
          -9223372036854775808 -9223372036854775809
          99999999999999999999 -99999999999999999999))
(defrule probe (bounds $?values) =>
  (printout t (length$ ?values) " " ?values crlf)
  (foreach ?value ?values (printout t "[" (integerp ?value) "]"))
  (printout t crlf)
  (printout t (create$ 99999999999999999999 -99999999999999999999) crlf))
