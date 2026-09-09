(deffacts seed (word abc) (text "wxyz"))
(defrule probe (word ?symbol) (text ?string) =>
  (printout t (str-length ?symbol) ":" (str-length ?string) ":"
    (integerp (str-length ?symbol)) crlf))
