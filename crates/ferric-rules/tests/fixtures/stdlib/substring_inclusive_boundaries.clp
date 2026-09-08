(defrule probe =>
 (printout t "[" (sub-string 0 1 "abc") "]:[" (sub-string 0 3 "abc") "]:["
  (sub-string 2 2 "abc") "]:[" (sub-string 3 3 "abc") "]:["
  (sub-string 3 4 "abc") "]:[" (sub-string 4 4 "abc") "]" crlf))
