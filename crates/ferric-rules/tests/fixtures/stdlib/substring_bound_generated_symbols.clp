(deffacts input (word abc))
(deffunction take (?start ?end ?text) (sub-string ?start ?end ?text))
(defrule probe (word ?word) =>
 (printout t "[" (sub-string 0 2 ?word) "]:["
  (sub-string 0 99 (sym-cat "alpha" "-" "beta")) "]:["
  (sub-string 2 3 (sym-cat "a b")) "]:[" (take -5 2 ?word) "]:"
  (stringp (take 0 2 ?word)) crlf))
