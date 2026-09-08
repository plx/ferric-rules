(defrule probe =>
 (printout t (str-index a banana) ":" (str-index "na" banana) ":" (str-index ana "banana") ":"
  (str-index "banana" "banana") ":" (str-index "a" "banana") ":" (str-index "z" "banana") ":"
  (str-index "abc" "ab") ":" (str-index "a" "") ":" (str-index "A" "abc") crlf))
