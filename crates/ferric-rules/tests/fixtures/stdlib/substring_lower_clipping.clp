(defrule probe =>
 (printout t "[" (sub-string 0 2 "abc") "]:[" (sub-string -1 2 "abc") "]:["
  (sub-string -100 2 "abc") "]:[" (sub-string -9223372036854775808 2 "abc") "]" crlf))
