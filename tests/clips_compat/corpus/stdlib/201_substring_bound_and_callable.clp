(deffunction take (?start ?end ?text) (sub-string ?start ?end ?text))
(deffacts input (slice 0 2 "abcd"))
(defrule probe (slice ?start ?end ?text) =>
 (printout t "[" (sub-string ?start ?end ?text) "]:[" (take -3 2 "abcd") "]:"
  (stringp (take 0 2 "abcd")) crlf))
