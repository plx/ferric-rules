(defrule probe =>
 (printout t "[" (sub-string 0 0 "abc") "]:[" (sub-string 0 -1 "abc") "]:["
  (sub-string -5 -1 "abc") "]:[" (sub-string -1 -2 "abc") "]:["
  (sub-string 3 2 "abc") "]:[" (sub-string 4 9 "abc") "]:["
  (sub-string 0 2 "") "]" crlf))
