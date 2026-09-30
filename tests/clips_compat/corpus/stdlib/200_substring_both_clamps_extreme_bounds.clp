(defrule probe =>
 (printout t "[" (sub-string -9223372036854775808 9223372036854775807 "abc") "]:["
  (sub-string 0 999 "abc") "]:[" (sub-string 2 9223372036854775807 "abc") "]:["
  (sub-string 9223372036854775807 9223372036854775807 "abc") "]:["
  (sub-string 1 -9223372036854775808 "abc") "]" crlf))
