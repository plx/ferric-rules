;; The field scanner of explode$ and string-to-field follows CLIPS's input
;; reader: a backspace inside a string erases the previous character, a
;; control character ends a symbol, and ETX ends the input.
;; Level: boundary
;; Covers: explode$, string-to-field
(defrule probe =>
  (bind ?bs (format nil "%c" 8))
  (bind ?etx (format nil "%c" 3))
  (bind ?quoted (explode$ (str-cat "\"ab" ?bs "c\" x")))
  (printout t (length$ ?quoted) " " (str-length (nth$ 1 ?quoted)) " " (nth$ 2 ?quoted) crlf)
  (bind ?stopped (explode$ (str-cat "a b" ?etx "c d")))
  (printout t (length$ ?stopped) " " ?stopped crlf)
  (bind ?symbols (explode$ (str-cat "ab" ?bs "c d")))
  (printout t (length$ ?symbols) " " (str-length (nth$ 1 ?symbols)) crlf)
  (bind ?field (string-to-field (str-cat "\"x" ?bs ?bs "yz\"")))
  (printout t (str-length ?field) " " ?field crlf))
